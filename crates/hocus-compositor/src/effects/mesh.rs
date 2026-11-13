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
}
