//! Triangles an effect draws in one call (#793), as the effects' shader
//! reads them ([`super::draw`], `SHAPES`): each vertex its place (output-
//! local, physical), its place across its shape, its colour (premultiplied)
//! and its shape (what it is, and up to three numbers of it). Shapes made
//! of quads (a flake, a clump, a soft box) shade each pixel; a fill takes
//! its vertices' colours, a fringe of transparent vertices smoothing its
//! edges.

use super::particles::FLOATS;

/// What a vertex's shape is: its first number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Its colour as it is.
    Fill = 0,
    /// A soft disc (a flake), white at its middle to nothing at its edge,
    /// three quarters at its stop (the shape's second number).
    Flake = 1,
    /// A clump of snow: an ellipse 1 by 0.8 of its quad, white at its
    /// upper left to blue at its edge; its radius in pixels.
    Clump = 2,
    /// A box (its half sizes, pixels) blurred by a Gaussian (its standard
    /// deviation, pixels; under half a pixel, sharp).
    Box = 3,
    /// A box with round corners (its half sizes and its corners' radius,
    /// pixels; its place across in pixels from its middle), its edges
    /// smoothed over a pixel: a stroke's piece is one whose radius is its
    /// half height (#799).
    Rounded = 4,
    /// An oval: its place across from -1 to 1 on its radii, its colour
    /// its vertices' (a linear gradient laid on its corners); its edge
    /// smoothed over the pixels its radius (the shape's second number)
    /// spans.
    Oval = 5,
    /// A glow from its middle to nothing at its edge, by the profile the
    /// shape's second number names ([`Glow`]).
    Glow = 6,
    /// A drop of rain as the map of the drops holds it (#802): a lens
    /// whose colour is its normal (red across, green down, from 0 to 1)
    /// and its thickness (blue), opaque but its rim (the board's sprite).
    Drop = 7,
    /// A lily pad (#803): a disc but a notch, its places across from -1
    /// to 1; its radius and the pixels a logical one (the shape's second
    /// and fourth numbers); a shadow (its third number 1) of its
    /// vertices' colour, else the leaf (the board's gradient from its upper
    /// left, its veins) as opaque as its vertices' alpha.
    Pad = 8,
}

/// The glows' profiles: how opaque from the middle out, the board's
/// gradients' stops (#799).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glow {
    /// A bulb's halo: 55 % at its middle, 32 % at 12 %, 10 % at 35 %,
    /// 2.5 % at 70 %, nothing at its edge.
    Halo = 0,
    /// A bulb's core: whole, 85 % at 30 %, 25 % at 60 %, nothing at its
    /// edge.
    Core = 1,
    /// A glint: whole, half at half, nothing at its edge.
    Glint = 2,
    /// A firefly's glow (#804): its own colours, warm white at its middle,
    /// 95 % lime at 8 %, 35 % green at 25 %, 8 % at 60 %, nothing at its
    /// edge; as opaque as its vertices' alpha.
    Firefly = 3,
    /// A firework's spark (#810): white at its middle, its vertices'
    /// colour at 90 % at 15 %, 22 % at 45 %, nothing at its edge (the
    /// board's sprite).
    Spark = 4,
    /// A firework's spark in the bar (#800, the board "Fenêtre
    /// maximisée"'s sprite): white at its middle, its vertices' colour at
    /// 90 % at 25 %, nothing at its edge.
    Bead = 5,
}

/// A column of a fill: its place across, and its rows from the top down
/// (place and colour).
pub type Column<const ROWS: usize> = (f32, [(f32, [f32; 4]); ROWS]);

/// Triangles, indexed.
#[derive(Debug, Default, Clone)]
pub struct Mesh {
    pub vertices: Vec<f32>,
    pub indices: Vec<u16>,
    /// The bounds of what they cover (`x0, y0, x1, y1`).
    bounds: Option<[f32; 4]>,
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// A vertex; `None` once the mesh holds as many as indices can tell.
    pub fn vertex(
        &mut self,
        at: [f32; 2],
        local: [f32; 2],
        colour: [f32; 4],
        shape: Shape,
        numbers: [f32; 3],
    ) -> Option<u16> {
        let n = self.vertices.len() / FLOATS;
        let index = u16::try_from(n).ok()?;
        self.vertices.extend_from_slice(&[
            at[0],
            at[1],
            local[0],
            local[1],
            colour[0],
            colour[1],
            colour[2],
            colour[3],
            shape as u8 as f32,
            numbers[0],
            numbers[1],
            numbers[2],
        ]);
        let [x, y] = at;
        self.bounds = Some(match self.bounds {
            None => [x, y, x, y],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        });
        Some(index)
    }

    /// Into `out`, the indices of its triangles that reach into `x0, y0,
    /// x1, y1` (its places' space): what a draw cut to it needs (#815).
    pub fn indices_within(&self, [x0, y0, x1, y1]: [f32; 4], out: &mut Vec<u16>) {
        out.clear();
        let at = |i: u16| {
            let i = usize::from(i) * FLOATS;
            match self.vertices.get(i..i + 2) {
                Some(&[x, y]) => Some((x, y)),
                _ => None,
            }
        };
        for t in self.indices.as_chunks::<3>().0 {
            let [i, j, k] = *t;
            let (Some(a), Some(b), Some(c)) = (at(i), at(j), at(k)) else {
                continue;
            };
            let (lo_x, hi_x) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
            let (lo_y, hi_y) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
            if lo_x <= x1 && hi_x >= x0 && lo_y <= y1 && hi_y >= y0 {
                out.extend_from_slice(t);
            }
        }
    }

    /// Its places moved by `x, y`.
    pub fn shift(&mut self, x: f32, y: f32) {
        for v in self.vertices.as_chunks_mut::<FLOATS>().0 {
            let [vx, vy, ..] = v;
            *vx += x;
            *vy += y;
        }
        if let Some([x0, y0, x1, y1]) = self.bounds {
            self.bounds = Some([x0 + x, y0 + y, x1 + x, y1 + y]);
        }
    }

    pub fn triangle(&mut self, a: u16, b: u16, c: u16) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    /// A quad of `shape`: its corners clockwise from the top left, and
    /// their places across it.
    pub fn quad(
        &mut self,
        corners: [[f32; 2]; 4],
        locals: [[f32; 2]; 4],
        colour: [f32; 4],
        shape: Shape,
        numbers: [f32; 3],
    ) {
        self.quad_of(corners, locals, [colour; 4], shape, numbers);
    }

    /// The same, each corner a colour of its own: what lies between
    /// mixed (a linear gradient).
    pub fn quad_of(
        &mut self,
        corners: [[f32; 2]; 4],
        locals: [[f32; 2]; 4],
        colours: [[f32; 4]; 4],
        shape: Shape,
        numbers: [f32; 3],
    ) {
        let mut at = [0u16; 4];
        for (slot, ((c, l), colour)) in at
            .iter_mut()
            .zip(corners.into_iter().zip(locals).zip(colours))
        {
            match self.vertex(c, l, colour, shape, numbers) {
                Some(i) => *slot = i,
                None => return,
            }
        }
        self.triangle(at[0], at[1], at[2]);
        self.triangle(at[0], at[2], at[3]);
    }

    /// An upright quad around `centre` reaching `half` (pixels) each way,
    /// its places across it from `-local` to `local`.
    pub fn square(
        &mut self,
        centre: [f32; 2],
        half: [f32; 2],
        local: [f32; 2],
        colour: [f32; 4],
        shape: Shape,
        numbers: [f32; 3],
    ) {
        let [cx, cy] = centre;
        let [hx, hy] = half;
        let [lx, ly] = local;
        self.quad(
            [
                [cx - hx, cy - hy],
                [cx + hx, cy - hy],
                [cx + hx, cy + hy],
                [cx - hx, cy + hy],
            ],
            [[-lx, -ly], [lx, -ly], [lx, ly], [-lx, ly]],
            colour,
            shape,
            numbers,
        );
    }

    /// A fill between columns: each column its place across (`x`) and its
    /// rows from the top down (place and colour), as many rows each; the
    /// bands between two rows of two columns side by side filled.
    #[expect(
        clippy::indexing_slicing,
        reason = "r and r + 1 are rows of arrays of ROWS"
    )]
    pub fn columns<const ROWS: usize>(&mut self, columns: &[Column<ROWS>]) {
        let mut last: Option<[u16; ROWS]> = None;
        for (x, rows) in columns {
            let mut at = [0u16; ROWS];
            for (slot, (y, colour)) in at.iter_mut().zip(rows) {
                match self.vertex([*x, *y], [0.0, 0.0], *colour, Shape::Fill, [0.0; 3]) {
                    Some(i) => *slot = i,
                    None => return,
                }
            }
            if let Some(prev) = last {
                for r in 0..ROWS - 1 {
                    self.triangle(prev[r], at[r], at[r + 1]);
                    self.triangle(prev[r], at[r + 1], prev[r + 1]);
                }
            }
            last = Some(at);
        }
    }

    /// What it covers, in whole pixels (`x, y, w, h`), when anything.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        let [x0, y0, x1, y1] = self.bounds?;
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        Some([x0, y0, x1.ceil() as i32 - x0, y1.ceil() as i32 - y0])
    }
}

/// Where `new` draws other than `old` did (#815), in whole pixels: each
/// triangle of either whose corners moved or changed, at its place in
/// both, those side by side joined; `out` gets them. Nothing when they
/// are the same.
pub fn changes(old: &Mesh, new: &Mesh, out: &mut Vec<[i32; 4]>) {
    if old.indices == new.indices && old.vertices == new.vertices {
        return;
    }
    fn corner(m: &Mesh, i: u16) -> Option<&[f32]> {
        let i = usize::from(i) * FLOATS;
        m.vertices.get(i..i + FLOATS)
    }
    // Each vertex the same in both, or not.
    let same: Vec<bool> = old
        .vertices
        .as_chunks::<FLOATS>()
        .0
        .iter()
        .zip(new.vertices.as_chunks::<FLOATS>().0)
        .map(|(a, b)| a == b)
        .collect();
    let bounds = |m: &Mesh, t: &[u16; 3]| -> Option<[i32; 4]> {
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for i in t {
            let v = corner(m, *i)?;
            let (x, y) = (*v.first()?, *v.get(1)?);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        // A pixel more each way: a fringe's last pixel, a soft shape's.
        let (x0, y0) = (b[0].floor() as i32 - 1, b[1].floor() as i32 - 1);
        Some([
            x0,
            y0,
            b[2].ceil() as i32 + 1 - x0,
            b[3].ceil() as i32 + 1 - y0,
        ])
    };
    let mut add = |r: [i32; 4]| {
        // Joined to the last when that covers little more than both.
        if let Some(last) = out.last_mut() {
            let x0 = last[0].min(r[0]);
            let y0 = last[1].min(r[1]);
            let x1 = (last[0] + last[2]).max(r[0] + r[2]);
            let y1 = (last[1] + last[3]).max(r[1] + r[3]);
            let joined = i64::from(x1 - x0) * i64::from(y1 - y0);
            let apart = i64::from(last[2]) * i64::from(last[3]) + i64::from(r[2]) * i64::from(r[3]);
            if joined <= apart + apart / 4 {
                *last = [x0, y0, x1 - x0, y1 - y0];
                return;
            }
        }
        out.push(r);
    };
    let (a, b) = (
        old.indices.as_chunks::<3>().0,
        new.indices.as_chunks::<3>().0,
    );
    let n = a.len().max(b.len());
    let (mut a, mut b) = (a.iter(), b.iter());
    for _ in 0..n {
        let (ta, tb) = (a.next(), b.next());
        if let (Some(ta), Some(tb)) = (ta, tb)
            && ta == tb
            && ta
                .iter()
                .all(|i| same.get(usize::from(*i)).copied().unwrap_or(false))
        {
            continue;
        }
        if let Some(r) = ta.and_then(|t| bounds(old, t)) {
            add(r);
        }
        if let Some(r) = tb.and_then(|t| bounds(new, t)) {
            add(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_and_columns_share_their_vertices() {
        let mut m = Mesh::default();
        assert!(m.is_empty() && m.bounds().is_none());
        m.square(
            [10.0, 10.0],
            [2.0, 3.0],
            [1.0, 1.0],
            [1.0; 4],
            Shape::Flake,
            [0.45, 0.0, 0.0],
        );
        assert_eq!((m.vertices.len() / FLOATS, m.indices.len()), (4, 6));
        assert_eq!(m.bounds(), Some([8, 7, 4, 6]));
        let white = [1.0; 4];
        m.columns(&[
            (0.0, [(0.0, white), (1.0, white), (2.0, white)]),
            (1.0, [(0.0, white), (1.0, white), (2.0, white)]),
            (2.5, [(0.5, white), (1.0, white), (2.0, white)]),
        ]);
        // Three vertices a column, two bands of two triangles between two.
        assert_eq!(m.vertices.len() / FLOATS, 4 + 9);
        assert_eq!(m.indices.len(), 6 + 2 * 2 * 2 * 3);
        assert_eq!(m.bounds(), Some([0, 0, 12, 13]));
    }

    #[test]
    fn what_changed_is_where_a_shape_moved_alone() {
        let flake = |m: &mut Mesh, x: f32| {
            m.square(
                [x, 10.0],
                [2.0, 2.0],
                [1.0, 1.0],
                [1.0; 4],
                Shape::Flake,
                [0.45, 0.0, 0.0],
            );
        };
        let (mut a, mut b) = (Mesh::default(), Mesh::default());
        for x in [10.0, 100.0, 200.0] {
            flake(&mut a, x);
        }
        let mut out = Vec::new();
        changes(&a, &a.clone(), &mut out);
        assert!(out.is_empty());
        for x in [10.0, 101.0, 200.0] {
            flake(&mut b, x);
        }
        changes(&a, &b, &mut out);
        // The middle one, where it was and where it is, in one.
        assert_eq!(out, vec![[97, 7, 7, 6]]);
        // One more: it too.
        flake(&mut b, 300.0);
        out.clear();
        changes(&a, &b, &mut out);
        assert_eq!(out, vec![[97, 7, 7, 6], [297, 7, 6, 6]]);
    }
}
