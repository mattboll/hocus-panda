//! Wobbly windows (docs/design/windows.md §6): while a window moves it
//! bends like jelly, the point under the pointer following it exactly and
//! the far side lagging, then settles with a small bounce once the pointer
//! stops.
//!
//! The model: a grid of points over the window, each displaced from where
//! the rigid window has it and pulled back by a damped spring. Springs are
//! stiffest where the window is held and softer away from it, so the far
//! side lags; each point is also pulled toward its neighbours, which keeps
//! the sheet smooth; the hand holds a soft patch rather than one point, so
//! there is no crease under it; and no point strays further than a share
//! of the window's size. Integrated at a fixed step from the frame clock's
//! time, so a frame at any time gives the same shape.
//!
//! Drawn by the GPU: the window (its rim and shadow too) is drawn into a
//! picture, which is drawn again on a mesh bent by the grid. With pixman,
//! the window moves rigidly.

use std::{any::Any, cell::RefCell, rc::Rc, time::Duration};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Frame, Offscreen, Renderer,
            element::{Element, Id, Kind, RenderElement},
            gles::{Capability, GlesError, GlesFrame, GlesRenderer, GlesTexture, ffi},
            pixman::PixmanRenderer,
            utils::{CommitCounter, DamageSet, OpaqueRegions},
        },
    },
    utils::{Buffer as BufferCoords, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

/// Target size of a cell of the grid, logical pixels, and how many cells
/// a side has at least and at most.
const CELL: f64 = 28.0;
const CELLS: (usize, usize) = (3, 40);
/// The springs' stiffness where the window is held (per second squared),
/// their damping ratio (below 1, a bounce), and how much neighbours pull.
const STIFFNESS: f64 = 260.0;
const DAMPING: f64 = 0.45;
const COUPLING: f64 = 110.0;
/// Away from the hand, springs soften to this share of their stiffness.
const SOFTEST: f64 = 0.45;
/// The patch the hand holds, logical pixels.
const GRIP: f64 = 120.0;
/// No point strays further than this share of the window's size.
const MAX_BEND: f64 = 0.15;
/// The integration step, and the longest time caught up in one frame.
const STEP: Duration = Duration::from_micros(4167);
const CATCH_UP: Duration = Duration::from_millis(50);
/// Below these, a point has settled: logical pixels, and per second.
const REST: f64 = 0.5;
const REST_SPEED: f64 = 6.0;
/// The bent mesh drawn: about this many logical pixels per piece, and at
/// most this many pieces a side.
const MESH_STEP: f64 = 16.0;
const MESH_MAX: usize = 48;

/// A window bending.
#[derive(Debug)]
pub struct Jelly {
    /// Cells across and down; the grid has one more point each way.
    gx: usize,
    gy: usize,
    /// The window's size, logical.
    w: f64,
    h: f64,
    /// Each point's displacement and velocity, logical.
    d: Vec<(f64, f64)>,
    v: Vec<(f64, f64)>,
    stiffness: Vec<f64>,
    /// How firmly the hand holds each point: 1 under it.
    hold: Vec<f64>,
    /// The point under the pointer; none once let go.
    anchor: Option<usize>,
    last: Duration,
    /// The picture of the window, kept from frame to frame while its size
    /// stays: the renderer's texture, its size.
    pub(crate) picture: RefCell<Option<(Size<i32, BufferCoords>, Rc<dyn Any>)>>,
    pub(crate) id: Id,
    commits: RefCell<CommitCounter>,
}

impl Jelly {
    /// A window `size` large held at `held` (from its corner, logical).
    pub fn new(size: (f64, f64), held: (f64, f64), now: Duration) -> Self {
        let (w, h) = (size.0.max(1.0), size.1.max(1.0));
        let cells = |len: f64| ((len / CELL).round() as usize).clamp(CELLS.0, CELLS.1);
        let (gx, gy) = (cells(w), cells(h));
        let n = (gx + 1) * (gy + 1);
        let ai = ((held.0 / w * gx as f64).round().max(0.0) as usize).min(gx);
        let aj = ((held.1 / h * gy as f64).round().max(0.0) as usize).min(gy);
        let reach = w.max(h) * 0.6;
        let mut stiffness = Vec::with_capacity(n);
        let mut hold = Vec::with_capacity(n);
        for j in 0..=gy {
            for i in 0..=gx {
                let dx = (i as f64 - ai as f64) * w / gx as f64;
                let dy = (j as f64 - aj as f64) * h / gy as f64;
                let d = dx.hypot(dy);
                stiffness.push(STIFFNESS * (SOFTEST + (1.0 - SOFTEST) * (-d / reach).exp()));
                hold.push((-(d * d) / (GRIP * GRIP)).exp());
            }
        }
        Self {
            gx,
            gy,
            w,
            h,
            d: vec![(0.0, 0.0); n],
            v: vec![(0.0, 0.0); n],
            stiffness,
            hold,
            anchor: Some(aj * (gx + 1) + ai),
            last: now,
            picture: RefCell::new(None),
            id: Id::new(),
            commits: RefCell::new(CommitCounter::default()),
        }
    }

    /// The window's size it was made for.
    pub fn size(&self) -> (f64, f64) {
        (self.w, self.h)
    }

    /// The window moved by (`dx`, `dy`): its points stay where they were,
    /// the springs will bring them along; the held one goes with the hand.
    pub fn moved(&mut self, dx: f64, dy: f64, now: Duration) {
        self.advance(now);
        for (k, d) in self.d.iter_mut().enumerate() {
            if Some(k) != self.anchor {
                d.0 -= dx;
                d.1 -= dy;
            }
        }
        self.clamp();
    }

    /// Nothing holds it any more: it settles.
    pub fn release(&mut self) {
        self.anchor = None;
    }

    pub fn is_held(&self) -> bool {
        self.anchor.is_some()
    }

    fn clamp(&mut self) {
        let (mx, my) = (self.w * MAX_BEND, self.h * MAX_BEND);
        for d in &mut self.d {
            d.0 = d.0.clamp(-mx, mx);
            d.1 = d.1.clamp(-my, my);
        }
    }

    /// The springs, up to `now`.
    pub fn advance(&mut self, now: Duration) {
        let mut left = now.saturating_sub(self.last).min(CATCH_UP);
        self.last = now;
        while !left.is_zero() {
            let dt = left.min(STEP);
            left -= dt;
            self.step(dt.as_secs_f64());
        }
    }

    fn step(&mut self, dt: f64) {
        let n = self.gx + 1;
        let (mx, my) = (self.w * MAX_BEND, self.h * MAX_BEND);
        for j in 0..=self.gy {
            for i in 0..=self.gx {
                let k = j * n + i;
                if Some(k) == self.anchor {
                    self.d[k] = (0.0, 0.0);
                    self.v[k] = (0.0, 0.0);
                    continue;
                }
                let (dx, dy) = self.d[k];
                // The neighbours' pull keeps the sheet smooth.
                let mut pull = (0.0, 0.0);
                let neighbours = [
                    (i > 0).then(|| k - 1),
                    (i < self.gx).then(|| k + 1),
                    (j > 0).then(|| k - n),
                    (j < self.gy).then(|| k + n),
                ];
                for o in neighbours.into_iter().flatten() {
                    pull.0 += self.d[o].0 - dx;
                    pull.1 += self.d[o].1 - dy;
                }
                let stiff = self.stiffness[k];
                let damp = 2.0 * DAMPING * stiff.sqrt();
                let (vx, vy) = self.v[k];
                let ax = -stiff * dx - damp * vx + COUPLING * pull.0;
                let ay = -stiff * dy - damp * vy + COUPLING * pull.1;
                let (mut vx, mut vy) = (vx + ax * dt, vy + ay * dt);
                let (mut nx, mut ny) = (dx + vx * dt, dy + vy * dt);
                if self.anchor.is_some() {
                    // Held: the patch under the hand follows it.
                    let keep = 1.0 - self.hold[k];
                    nx *= keep;
                    ny *= keep;
                    vx *= keep;
                    vy *= keep;
                }
                self.v[k] = (vx, vy);
                self.d[k] = (nx.clamp(-mx, mx), ny.clamp(-my, my));
            }
        }
    }

    /// Let go, and back at rest.
    pub fn settled(&self) -> bool {
        self.anchor.is_none()
            && self
                .d
                .iter()
                .zip(&self.v)
                .all(|(d, v)| d.0.hypot(d.1) < REST && v.0.hypot(v.1) < REST_SPEED)
    }

    /// The largest displacement, logical.
    pub fn largest(&self) -> f64 {
        self.d.iter().map(|d| d.0.hypot(d.1)).fold(0.0, f64::max)
    }

    /// The displacement at (`u`, `v`) across the window, from 0 to 1,
    /// between the grid's points.
    pub fn at(&self, u: f64, v: f64) -> (f64, f64) {
        let x = (u * self.gx as f64).clamp(0.0, self.gx as f64);
        let y = (v * self.gy as f64).clamp(0.0, self.gy as f64);
        let i = (x as usize).min(self.gx - 1);
        let j = (y as usize).min(self.gy - 1);
        let (fu, fv) = (x - i as f64, y - j as f64);
        let n = self.gx + 1;
        let a = self.d[j * n + i];
        let b = self.d[j * n + i + 1];
        let c = self.d[(j + 1) * n + i];
        let d = self.d[(j + 1) * n + i + 1];
        let mix = |a: f64, b: f64, c: f64, d: f64| {
            (1.0 - fu) * (1.0 - fv) * a + fu * (1.0 - fv) * b + (1.0 - fu) * fv * c + fu * fv * d
        };
        (mix(a.0, b.0, c.0, d.0), mix(a.1, b.1, c.1, d.1))
    }

    /// A new commit for the bent element: it changes every frame.
    fn next_commit(&self) -> CommitCounter {
        let mut c = self.commits.borrow_mut();
        c.increment();
        *c
    }
}

/// The window's picture drawn on the bent mesh.
#[derive(Debug)]
pub struct BentElement {
    id: Id,
    commit: CommitCounter,
    /// Physical, output-local: the mesh's bounds.
    geometry: Rectangle<i32, Physical>,
    /// Triangles, `x, y, u, v` per vertex: output-local physical positions,
    /// and coordinates in the picture.
    vertices: Vec<f32>,
    texture: Rc<dyn Any>,
    alpha: f32,
}

/// What a window's picture is: where it is, output-local and logical,
/// and how far from the window's corner it starts.
#[derive(Debug, Clone, Copy)]
pub struct Picture {
    /// The window's geometry, output-local, logical.
    pub window: Rectangle<f64, Logical>,
    /// The picture's own rectangle, from the window's corner: rim and
    /// shadow included.
    pub extent: Rectangle<f64, Logical>,
    pub scale: f64,
}

impl Picture {
    /// The picture's size in pixels.
    pub fn buffer_size(&self) -> Size<i32, BufferCoords> {
        let s = self
            .extent
            .size
            .to_physical(self.scale)
            .to_i32_ceil::<i32>();
        (s.w.max(1), s.h.max(1)).into()
    }
}

/// The bent mesh of `jelly` over `picture`.
fn mesh(jelly: &Jelly, picture: &Picture, flip_v: bool) -> (Vec<f32>, Rectangle<i32, Physical>) {
    let e = picture.extent;
    let pieces = |len: f64| ((len / MESH_STEP).ceil() as usize).clamp(1, MESH_MAX);
    let (nx, ny) = (pieces(e.size.w), pieces(e.size.h));
    let scale = picture.scale;
    let point = |i: usize, j: usize| {
        let s = i as f64 / nx as f64;
        let t = j as f64 / ny as f64;
        // From the window's corner, logical.
        let px = e.loc.x + s * e.size.w;
        let py = e.loc.y + t * e.size.h;
        let (dx, dy) = jelly.at(px / jelly.w, py / jelly.h);
        let x = (picture.window.loc.x + px + dx) * scale;
        let y = (picture.window.loc.y + py + dy) * scale;
        let v = if flip_v { 1.0 - t } else { t };
        (x, y, s, v)
    };
    let mut vertices = Vec::with_capacity(nx * ny * 24);
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    let grid: Vec<(f64, f64, f64, f64)> = (0..=ny)
        .flat_map(|j| (0..=nx).map(move |i| (i, j)))
        .map(|(i, j)| point(i, j))
        .collect();
    for p in &grid {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    let at = |i: usize, j: usize| grid[j * (nx + 1) + i];
    for j in 0..ny {
        for i in 0..nx {
            for (a, b) in [
                (i, j),
                (i + 1, j),
                (i, j + 1),
                (i + 1, j),
                (i + 1, j + 1),
                (i, j + 1),
            ] {
                let p = at(a, b);
                vertices.extend_from_slice(&[p.0 as f32, p.1 as f32, p.2 as f32, p.3 as f32]);
            }
        }
    }
    let geometry = Rectangle::from_extremities(
        (lo.0.floor() as i32, lo.1.floor() as i32),
        (hi.0.ceil() as i32, hi.1.ceil() as i32),
    );
    (vertices, geometry)
}

/// Renderers that can bend a window.
pub trait Bend: Renderer + Sized {
    /// Draws `parts` (output-local, as if the window's corner were at the
    /// picture's) into the window's picture, and gives the element drawing
    /// it bent. `None`: the window is drawn rigid.
    fn bend<E: RenderElement<Self>>(
        &mut self,
        jelly: &Jelly,
        picture: &Picture,
        parts: &[E],
        alpha: f32,
    ) -> Option<BentElement>;

    fn draw_bent(frame: &mut Self::Frame<'_, '_>, bent: &BentElement) -> Result<(), Self::Error>;
}

impl Bend for PixmanRenderer {
    fn bend<E: RenderElement<Self>>(
        &mut self,
        _jelly: &Jelly,
        _picture: &Picture,
        _parts: &[E],
        _alpha: f32,
    ) -> Option<BentElement> {
        None
    }

    fn draw_bent(_frame: &mut Self::Frame<'_, '_>, _bent: &BentElement) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The program drawing a bent picture.
#[derive(Debug, Clone, Copy)]
struct MeshProgram {
    program: ffi::types::GLuint,
    position: ffi::types::GLint,
    texcoord: ffi::types::GLint,
    matrix: ffi::types::GLint,
    tex: ffi::types::GLint,
    alpha: ffi::types::GLint,
}

const VERTEX: &str = "#version 100
uniform mat3 matrix;
attribute vec2 position;
attribute vec2 texcoord;
varying vec2 v_tex;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_tex = texcoord;
}
";

const FRAGMENT: &str = "#version 100
precision mediump float;
uniform sampler2D tex;
uniform float alpha;
varying vec2 v_tex;
void main() {
    gl_FragColor = texture2D(tex, v_tex) * alpha;
}
";

/// Compiles and links the mesh program in the current context.
///
/// # Safety
///
/// The renderer's context must be current, as `with_context` makes it.
#[allow(unsafe_code)]
unsafe fn compile(gl: &ffi::Gles2) -> Option<MeshProgram> {
    // SAFETY: called with the context current; every pointer handed to GL
    // points to a live local for the duration of the call.
    unsafe {
        let shader = |kind: ffi::types::GLenum, src: &str| {
            let id = gl.CreateShader(kind);
            let ptr = src.as_ptr() as *const ffi::types::GLchar;
            let len = src.len() as ffi::types::GLint;
            gl.ShaderSource(id, 1, &ptr, &len);
            gl.CompileShader(id);
            let mut ok = 0;
            gl.GetShaderiv(id, ffi::COMPILE_STATUS, &mut ok);
            if ok == 0 {
                gl.DeleteShader(id);
                None
            } else {
                Some(id)
            }
        };
        let vs = shader(ffi::VERTEX_SHADER, VERTEX)?;
        let Some(fs) = shader(ffi::FRAGMENT_SHADER, FRAGMENT) else {
            gl.DeleteShader(vs);
            return None;
        };
        let program = gl.CreateProgram();
        gl.AttachShader(program, vs);
        gl.AttachShader(program, fs);
        gl.LinkProgram(program);
        gl.DetachShader(program, vs);
        gl.DetachShader(program, fs);
        gl.DeleteShader(vs);
        gl.DeleteShader(fs);
        let mut ok = 0;
        gl.GetProgramiv(program, ffi::LINK_STATUS, &mut ok);
        if ok == 0 {
            gl.DeleteProgram(program);
            return None;
        }
        let attrib = |name: &[u8]| gl.GetAttribLocation(program, name.as_ptr() as *const _);
        let uniform = |name: &[u8]| gl.GetUniformLocation(program, name.as_ptr() as *const _);
        Some(MeshProgram {
            program,
            position: attrib(b"position\0"),
            texcoord: attrib(b"texcoord\0"),
            matrix: uniform(b"matrix\0"),
            tex: uniform(b"tex\0"),
            alpha: uniform(b"alpha\0"),
        })
    }
}

/// The mesh program of a renderer's context, compiled once; `None` when
/// it does not compile, and the windows then move rigid.
#[derive(Debug)]
struct Compiled(Option<MeshProgram>);

impl Bend for GlesRenderer {
    fn bend<E: RenderElement<Self>>(
        &mut self,
        jelly: &Jelly,
        picture: &Picture,
        parts: &[E],
        alpha: f32,
    ) -> Option<BentElement> {
        if self.egl_context().user_data().get::<Compiled>().is_none() {
            #[allow(unsafe_code)]
            // SAFETY: `with_context` makes the renderer's context current.
            let program = self
                .with_context(|gl| unsafe { compile(gl) })
                .ok()
                .flatten();
            if program.is_none() {
                tracing::warn!("the wobbly windows' program does not compile: windows move rigid");
            }
            self.egl_context()
                .user_data()
                .insert_if_missing(|| Compiled(program));
        }
        self.egl_context()
            .user_data()
            .get::<Compiled>()
            .and_then(|c| c.0)?;
        let size = picture.buffer_size();
        let texture = {
            let mut kept = jelly.picture.borrow_mut();
            let reuse = kept
                .as_ref()
                .filter(|(s, _)| *s == size)
                .and_then(|(_, t)| t.downcast_ref::<GlesTexture>().cloned());
            match reuse {
                Some(texture) => texture,
                None => {
                    let texture: GlesTexture =
                        Offscreen::<GlesTexture>::create_buffer(self, Fourcc::Abgr8888, size)
                            .map_err(|err| tracing::warn!(%err, "no picture for a wobbly window"))
                            .ok()?;
                    *kept = Some((size, Rc::new(texture.clone())));
                    texture
                }
            }
        };
        let mut target = texture.clone();
        if let Err(err) = draw_picture(self, &mut target, picture, parts) {
            tracing::warn!(%err, "could not draw a wobbly window's picture");
            return None;
        }
        let (vertices, geometry) = mesh(jelly, picture, false);
        Some(BentElement {
            id: jelly.id.clone(),
            commit: jelly.next_commit(),
            geometry,
            vertices,
            texture: Rc::new(texture),
            alpha,
        })
    }

    fn draw_bent(frame: &mut GlesFrame<'_, '_>, bent: &BentElement) -> Result<(), GlesError> {
        let Some(texture) = bent.texture.downcast_ref::<GlesTexture>() else {
            return Ok(());
        };
        let Some(program) = frame
            .egl_context()
            .user_data()
            .get::<Compiled>()
            .and_then(|c| c.0)
        else {
            return Ok(());
        };
        let projection = *frame.projection();
        let instancing = frame.capabilities().contains(&Capability::Instancing);
        let vertices = &bent.vertices;
        let count = (vertices.len() / 4) as ffi::types::GLsizei;
        let tex = texture.tex_id();
        frame.with_context(|gl| {
            #[allow(unsafe_code)]
            // SAFETY: the frame's context is current; the vertices outlive
            // the draw call, and every attribute enabled here is disabled
            // again, as smithay's own draws expect.
            unsafe {
                gl.UseProgram(program.program);
                gl.ActiveTexture(ffi::TEXTURE0);
                gl.BindTexture(ffi::TEXTURE_2D, tex);
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, ffi::LINEAR as i32);
                gl.Uniform1i(program.tex, 0);
                gl.Uniform1f(program.alpha, bent.alpha);
                gl.UniformMatrix3fv(program.matrix, 1, ffi::FALSE, projection.as_ptr());
                gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
                let stride = (4 * std::mem::size_of::<f32>()) as ffi::types::GLsizei;
                let (pos, uv) = (program.position as u32, program.texcoord as u32);
                gl.EnableVertexAttribArray(pos);
                gl.EnableVertexAttribArray(uv);
                if instancing {
                    gl.VertexAttribDivisor(pos, 0);
                    gl.VertexAttribDivisor(uv, 0);
                }
                gl.VertexAttribPointer(
                    pos,
                    2,
                    ffi::FLOAT,
                    ffi::FALSE,
                    stride,
                    vertices.as_ptr() as *const _,
                );
                gl.VertexAttribPointer(
                    uv,
                    2,
                    ffi::FLOAT,
                    ffi::FALSE,
                    stride,
                    vertices.as_ptr().add(2) as *const _,
                );
                gl.DrawArrays(ffi::TRIANGLES, 0, count);
                gl.DisableVertexAttribArray(pos);
                gl.DisableVertexAttribArray(uv);
                gl.BindTexture(ffi::TEXTURE_2D, 0);
            }
        })
    }
}

/// Draws the window's parts into its picture.
fn draw_picture<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    target: &mut GlesTexture,
    picture: &Picture,
    parts: &[E],
) -> Result<(), GlesError> {
    let size = picture.buffer_size();
    let physical: Size<i32, Physical> = (size.w, size.h).into();
    let scale = Scale::from(picture.scale);
    let mut framebuffer = renderer.bind(target)?;
    let mut frame = renderer.render(&mut framebuffer, physical, Transform::Normal)?;
    let whole = Rectangle::from_size(physical);
    frame.clear(Color32F::TRANSPARENT, &[whole])?;
    for part in parts.iter().rev() {
        let dst = part.geometry(scale);
        let Some(seen) = dst.intersection(whole) else {
            continue;
        };
        let mut damage = seen;
        damage.loc -= dst.loc;
        part.draw(&mut frame, part.src(), dst, &[damage], &[])?;
    }
    let sync = frame.finish()?;
    drop(framebuffer);
    renderer.wait(&sync)
}

impl Element for BentElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        Rectangle::from_size((1.0, 1.0).into())
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn location(&self, _scale: Scale<f64>) -> Point<i32, Physical> {
        self.geometry.loc
    }

    fn transform(&self) -> Transform {
        Transform::Normal
    }

    fn damage_since(
        &self,
        _scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        if commit == Some(self.commit) {
            DamageSet::default()
        } else {
            DamageSet::from_slice(&[Rectangle::from_size(self.geometry.size)])
        }
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }

    fn alpha(&self) -> f32 {
        self.alpha
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R: Bend> RenderElement<R> for BentElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, BufferCoords>,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        R::draw_bent(frame, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_held_point_follows_and_the_far_side_lags() {
        let mut jelly = Jelly::new((800.0, 600.0), (0.0, 0.0), ms(0));
        for t in 1..=10 {
            jelly.moved(8.0, 0.0, ms(t * 16));
        }
        let (held, _) = jelly.at(0.0, 0.0);
        let (far, _) = jelly.at(1.0, 1.0);
        assert!(held.abs() < 0.5, "{held}");
        assert!(far < -10.0, "the far side lags: {far}");
        // Never further than its share of the size.
        assert!(jelly.largest() <= 800.0 * MAX_BEND + 1e-6);
    }

    #[test]
    fn let_go_it_bounces_and_settles() {
        let mut jelly = Jelly::new((800.0, 600.0), (400.0, 20.0), ms(0));
        for t in 1..=10 {
            jelly.moved(12.0, 4.0, ms(t * 16));
        }
        jelly.release();
        let start = ms(160);
        let mut overshot = false;
        let mut quiet = None;
        for step in 1..=150u64 {
            let now = start + ms(step * 8);
            jelly.advance(now);
            let (x, _) = jelly.at(0.0, 1.0);
            overshot |= x > 0.5;
            if quiet.is_none() && jelly.largest() < 3.0 {
                quiet = Some(now - start);
            }
            if jelly.settled() {
                break;
            }
        }
        assert!(overshot, "a small bounce");
        let quiet = quiet.unwrap_or(ms(9999));
        assert!(quiet <= ms(600), "still moving visibly after {quiet:?}");
        assert!(jelly.settled(), "settled within 1.2 s");
    }

    #[test]
    fn the_mesh_covers_the_picture_and_bends_with_the_jelly() {
        let mut jelly = Jelly::new((400.0, 300.0), (0.0, 0.0), ms(0));
        let picture = Picture {
            window: Rectangle::new((100.0, 100.0).into(), (400.0, 300.0).into()),
            extent: Rectangle::new((-20.0, -20.0).into(), (440.0, 360.0).into()),
            scale: 1.0,
        };
        let (still, geometry) = mesh(&jelly, &picture, false);
        assert_eq!(geometry, Rectangle::new((80, 80).into(), (440, 360).into()));
        assert_eq!(still.len() % 24, 0);
        jelly.moved(30.0, 0.0, ms(16));
        let (_, bent) = mesh(&jelly, &picture, false);
        assert!(bent.loc.x < geometry.loc.x || bent.size.w > geometry.size.w);
    }
}
