//! Pictures of a window that outlive what it shows (#34): closing,
//! minimizing and maximizing draw the window as it was while its client
//! has moved on. The surfaces' textures, already on the graphics card, are
//! kept by handle (nothing is copied) and drawn stretched as the motion
//! goes. smithay drops a surface's textures with the surface, so they are
//! taken as the toplevel goes, before.
//!
//! - Close: the picture dissolves from where the window was, its opening
//!   motion included; in the Matrix theme it decomposes into glyphs.
//! - Minimize: a magic lamp (genie) into its dock icon, or the column when
//!   the dock is hidden; restoring plays it back from there. In the Matrix
//!   theme a CRT switching off and on (`matrix_windows`).
//! - Maximize and fullscreen: the picture morphs from the old place to the
//!   new one and fades, the window drawn stretched beneath it. In the Matrix
//!   theme it springs there behind circuit traces, and snapping moves it so
//!   too.
//! - With reduced motion, each is a short fade where the window was.

use std::{any::Any, cell::OnceCell, collections::BTreeMap, collections::BTreeSet, rc::Rc};

use hocus_core::WindowId;
use hocus_shell::matrix::hash;
use hocus_ui::motion::{Animated, Curve, Easing, LARGE, SMALL, STANDARD};
use smithay::{
    backend::renderer::{
        ContextId, ImportAll, ImportMem, Renderer, Texture,
        element::{Id, Kind, texture::TextureRenderElement},
        utils::RendererSurfaceStateUserData,
    },
    desktop::Window,
    output::Output,
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, Size, Transform},
    wayland::{
        compositor::{SurfaceData, TraversalAction, with_states, with_surface_tree_downward},
        seat::WaylandFocus,
    },
};

use crate::{
    State,
    decoration::{Rounded, hairline_elements, shadow_elements},
    matrix,
    matrix_windows::{Layer, SWITCH, Shift, Switch},
    motion::{Look, WindowMotion},
    scene::SceneElement,
};

/// A surface of the window as it was drawn.
#[derive(Clone)]
struct Piece {
    /// The renderer's texture, by handle.
    texture: Rc<dyn Any>,
    /// From the window's geometry corner.
    at: Point<f64, Logical>,
    /// The part of the buffer shown, and the size it is shown at.
    src: Rectangle<f64, Logical>,
    size: Size<i32, Logical>,
    scale: i32,
    transform: Transform,
}

/// A window's surfaces as they were drawn, back first.
#[derive(Clone)]
pub struct Snapshot {
    pieces: Vec<Piece>,
    /// The window's geometry size.
    size: Size<f64, Logical>,
}

type Grab = Box<dyn Fn(&WlSurface, Point<f64, Logical>) -> Vec<Piece>>;

/// A window gone, dissolving.
struct Closing {
    id: WindowId,
    picture: Snapshot,
    /// Its geometry, global.
    frame: Rectangle<f64, Logical>,
    decorated: bool,
    opacity: f32,
    fade: Fade,
}

/// How a closing window goes.
enum Fade {
    /// It dissolves, shrinking back.
    Plain(WindowMotion),
    /// It decomposes into the Matrix theme's glyphs.
    Matrix(Box<matrix::Close>),
}

impl Closing {
    fn is_running(&self, now: std::time::Duration) -> bool {
        match &self.fade {
            Fade::Plain(motion) => motion.is_running(now),
            Fade::Matrix(close) => close.is_running(now),
        }
    }
}

/// How a window goes into its dock icon and comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Away {
    /// The magic lamp.
    Lamp,
    /// The Matrix theme's CRT switching off and on.
    Crt,
    /// A short fade where it is: reduced motion.
    Fade,
}

/// A window going into, or coming out of, its dock icon.
struct Genie {
    id: WindowId,
    picture: Snapshot,
    window: Rectangle<f64, Logical>,
    target: Rectangle<f64, Logical>,
    /// 0 the window, 1 in the target.
    progress: Animated,
    restoring: bool,
    away: Away,
    decorated: bool,
}

/// How a window changes place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Its picture morphs into the new place, fading.
    Morph,
    /// In the Matrix theme's way.
    Matrix(Shift),
    /// At its new place at once, its old picture fading where it was:
    /// reduced motion.
    Fade,
}

/// A window between two places: maximized, fullscreen, back; snapped in
/// the Matrix theme.
struct Morph {
    id: WindowId,
    picture: Snapshot,
    from: Rectangle<f64, Logical>,
    /// Where it was asked to go, global: the client takes its time to
    /// follow, the motion does not wait.
    to: Option<Rectangle<f64, Logical>>,
    progress: Animated,
    change: Change,
    seed: u64,
}

/// Where a window changing place is drawn now, global, and its old
/// picture over it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MorphView {
    pub window: Rectangle<f64, Logical>,
    pub picture: Rectangle<f64, Logical>,
    pub alpha: f32,
}

impl Morph {
    fn view(&self, live: Rectangle<f64, Logical>, now: std::time::Duration) -> MorphView {
        let p = self.progress.value(now);
        let to = self.to.unwrap_or(live);
        match self.change {
            Change::Morph => {
                let r = lerp_rect(self.from, to, f64::from(p));
                MorphView {
                    window: r,
                    picture: r,
                    alpha: 1.0 - p,
                }
            }
            Change::Matrix(shift) => {
                let r = lerp_rect(self.from, to, f64::from(shift.geometry(p)));
                MorphView {
                    window: r,
                    picture: r,
                    alpha: shift.picture_alpha(p),
                }
            }
            Change::Fade => MorphView {
                window: live,
                picture: self.from,
                alpha: 1.0 - p,
            },
        }
    }
}

#[derive(Default)]
pub struct Snapshots {
    /// Takes a surface tree's textures: made by the first drawing, which
    /// knows the renderer's texture type.
    grab: OnceCell<Grab>,
    closing: Vec<Closing>,
    genies: Vec<Genie>,
    morphs: Vec<Morph>,
    /// Minimized windows, with their picture and where they went, for the
    /// way back.
    away: BTreeMap<WindowId, (Snapshot, Rectangle<f64, Logical>)>,
    minimized: BTreeSet<WindowId>,
    /// Windows whose surface went before their toplevel (a client that
    /// quits or crashes): their picture, taken as the surface went.
    last: BTreeMap<WindowId, Snapshot>,
}

impl std::fmt::Debug for Snapshots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshots")
            .field("closing", &self.closing.len())
            .field("genies", &self.genies.len())
            .field("morphs", &self.morphs.len())
            .field("away", &self.away.len())
            .finish_non_exhaustive()
    }
}

fn grabber<T: Texture + Clone + 'static>(context: ContextId<T>) -> Grab {
    Box::new(move |surface, origin| {
        let mut pieces = Vec::new();
        let mut take = |states: &SurfaceData, at: Point<f64, Logical>| {
            let Some(data) = states.data_map.get::<RendererSurfaceStateUserData>() else {
                return;
            };
            let Ok(data) = data.lock() else {
                return;
            };
            let (Some(view), Some(texture)) = (data.view(), data.texture(context.clone())) else {
                return;
            };
            pieces.push(Piece {
                texture: Rc::new(texture.clone()),
                at: at + view.offset.to_f64(),
                src: view.src,
                size: view.dst,
                scale: data.buffer_scale(),
                transform: data.buffer_transform(),
            });
        };
        if surface.is_alive() {
            with_surface_tree_downward(
                surface,
                origin,
                |_, states, at| {
                    let view = states
                        .data_map
                        .get::<RendererSurfaceStateUserData>()
                        .and_then(|d| d.lock().ok()?.view());
                    match view {
                        Some(view) => TraversalAction::DoChildren(*at + view.offset.to_f64()),
                        None => TraversalAction::SkipChildren,
                    }
                },
                |_, states, at| take(states, *at),
                |_, _, _| true,
            );
        } else {
            // Going: the tree is no longer walked, its own state is still
            // there.
            with_states(surface, |states| take(states, origin));
        }
        pieces
    })
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_rect(
    a: Rectangle<f64, Logical>,
    b: Rectangle<f64, Logical>,
    t: f64,
) -> Rectangle<f64, Logical> {
    Rectangle::new(
        (lerp(a.loc.x, b.loc.x, t), lerp(a.loc.y, b.loc.y, t)).into(),
        (lerp(a.size.w, b.size.w, t), lerp(a.size.h, b.size.h, t)).into(),
    )
}

/// The magic lamp: where the window's row `v` (0 its top, 1 its bottom)
/// is at `p` (0 the window, 1 in the target): its height and its left and
/// right ends. The window narrows towards the target as a funnel, its far
/// end leading, then pours into it.
pub fn genie_row(
    window: Rectangle<f64, Logical>,
    target: Rectangle<f64, Logical>,
    v: f64,
    p: f64,
) -> (f64, f64, f64) {
    let (top, bottom) = (window.loc.y, window.loc.y + window.size.h);
    let (t_top, t_bottom) = (target.loc.y, target.loc.y + target.size.h);
    // Going up: the same, upside down.
    if t_top + t_bottom < top + bottom {
        let flip = |r: Rectangle<f64, Logical>| {
            Rectangle::new((r.loc.x, -(r.loc.y + r.size.h)).into(), r.size)
        };
        let (y, left, right) = genie_row(flip(window), flip(target), 1.0 - v, p);
        return (-y, left, right);
    }
    // The funnel forms first, then the window slides down it, its far
    // end ahead.
    let funnel = smoothstep(p / 0.4);
    let slide = Easing::InOutCubic.apply(((p - 0.2) / 0.8) as f32) as f64;
    let slide = (slide * (1.0 + 0.6 * v)).min(1.0);
    let y = lerp(top + v * window.size.h, t_top + v * target.size.h, slide);
    let narrow = if t_top > top {
        smoothstep((y - top) / (t_top - top))
    } else {
        1.0
    } * funnel;
    let left = lerp(window.loc.x, target.loc.x, narrow);
    let right = lerp(
        window.loc.x + window.size.w,
        target.loc.x + target.size.w,
        narrow,
    );
    (y, left, right)
}

/// A piece's part between rows `y0` and `y1` (from the window's top,
/// logical), drawn into `dst` (output-local).
fn piece_element<R>(
    renderer: &R,
    piece: &Piece,
    rows: Option<(f64, f64)>,
    dst: Rectangle<i32, Logical>,
    scale: f64,
    alpha: f32,
) -> Option<TextureRenderElement<R::TextureId>>
where
    R: Renderer,
    R::TextureId: Clone + Texture + 'static,
{
    if dst.size.w <= 0 || dst.size.h <= 0 {
        return None;
    }
    let texture = piece.texture.downcast_ref::<R::TextureId>()?.clone();
    let mut src = piece.src;
    if let Some((y0, y1)) = rows {
        let h = f64::from(piece.size.h);
        let (a, b) = ((y0 - piece.at.y) / h, (y1 - piece.at.y) / h);
        src = Rectangle::new(
            (src.loc.x, src.loc.y + a * src.size.h).into(),
            (src.size.w, (b - a) * src.size.h).into(),
        );
    }
    Some(TextureRenderElement::from_static_texture(
        Id::new(),
        renderer.context_id(),
        dst.loc.to_f64().to_physical(scale),
        texture,
        piece.scale,
        piece.transform,
        Some(alpha),
        Some(src),
        Some(dst.size),
        None,
        Kind::Unspecified,
    ))
}

/// Whole-number edges, so neighbouring parts meet.
fn snap(x0: f64, y0: f64, x1: f64, y1: f64) -> Rectangle<i32, Logical> {
    let (x0, y0) = (x0.round() as i32, y0.round() as i32);
    let (x1, y1) = (x1.round() as i32, y1.round() as i32);
    Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into())
}

impl Snapshot {
    /// Its pieces stretched into `into` (output-local), front first.
    pub(crate) fn stretched<R>(
        &self,
        renderer: &R,
        into: Rectangle<f64, Logical>,
        scale: f64,
        alpha: f32,
    ) -> Vec<TextureRenderElement<R::TextureId>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        let (kx, ky) = (
            into.size.w / self.size.w.max(1.0),
            into.size.h / self.size.h.max(1.0),
        );
        self.pieces
            .iter()
            .rev()
            .filter_map(|p| {
                let x0 = into.loc.x + p.at.x * kx;
                let y0 = into.loc.y + p.at.y * ky;
                let dst = snap(
                    x0,
                    y0,
                    x0 + f64::from(p.size.w) * kx,
                    y0 + f64::from(p.size.h) * ky,
                );
                piece_element(renderer, p, None, dst, scale, alpha)
            })
            .collect()
    }

    /// The band of its pieces between `v0` and `v1` of its height (0 its
    /// top, 1 its bottom), stretched into `into` as [`Self::stretched`]
    /// does, and moved `dx` sideways.
    fn band<R>(
        &self,
        renderer: &R,
        into: Rectangle<f64, Logical>,
        (v0, v1): (f64, f64),
        dx: f64,
        scale: f64,
        alpha: f32,
    ) -> Vec<TextureRenderElement<R::TextureId>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        let (kx, ky) = (
            into.size.w / self.size.w.max(1.0),
            into.size.h / self.size.h.max(1.0),
        );
        let (b0, b1) = (v0 * self.size.h, v1 * self.size.h);
        self.pieces
            .iter()
            .rev()
            .filter_map(|p| {
                let top = p.at.y.max(b0);
                let bottom = (p.at.y + f64::from(p.size.h)).min(b1);
                if bottom <= top {
                    return None;
                }
                let x0 = into.loc.x + p.at.x * kx + dx;
                let dst = snap(
                    x0,
                    into.loc.y + top * ky,
                    x0 + f64::from(p.size.w) * kx,
                    into.loc.y + bottom * ky,
                );
                piece_element(renderer, p, Some((top, bottom)), dst, scale, alpha)
            })
            .collect()
    }
}

impl Genie {
    /// Its picture bent by the magic lamp at `p`, in strips; `origin` is the
    /// output's corner, global.
    fn elements<R>(
        &self,
        renderer: &R,
        p: f64,
        origin: Point<f64, Logical>,
        scale: f64,
        alpha: f32,
    ) -> Vec<TextureRenderElement<R::TextureId>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        let (window, target) = (self.window, self.target);
        bent(
            renderer,
            &self.picture,
            |v| genie_row(window, target, v, p),
            origin,
            scale,
            alpha,
        )
    }
}

/// `picture` bent row by row: `row(v)` says where the row at `v` of its
/// height (0 its top, 1 its bottom) goes, as its top, left and right,
/// global; drawn in strips, each the rectangle at its middle. `origin` is
/// the output's corner, global.
pub(crate) fn bent<R>(
    renderer: &R,
    picture: &Snapshot,
    row: impl Fn(f64) -> (f64, f64, f64),
    origin: Point<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<TextureRenderElement<R::TextureId>>
where
    R: Renderer,
    R::TextureId: Clone + Texture + 'static,
{
    let h = picture.size.h.max(1.0);
    let strips = ((h / 3.0) as usize).clamp(16, 160);
    let step = h / strips as f64;
    let mut out = Vec::new();
    for piece in picture.pieces.iter().rev() {
        let (top, bottom) = (piece.at.y, piece.at.y + f64::from(piece.size.h));
        let (u0, u1) = (
            piece.at.x / picture.size.w.max(1.0),
            (piece.at.x + f64::from(piece.size.w)) / picture.size.w.max(1.0),
        );
        // The rows the window's strips cut it at, its own ends too.
        let mut cuts = vec![top];
        let mut y = (top / step).floor() * step + step;
        while y < bottom {
            cuts.push(y);
            y += step;
        }
        cuts.push(bottom);
        for pair in cuts.windows(2) {
            let (y0, y1) = (pair[0], pair[1]);
            let (a, l0, r0) = row(y0 / h);
            let (b, l1, r1) = row(y1 / h);
            // A trapezoid, drawn as the rectangle at its middle.
            let (l, r) = ((l0 + l1) / 2.0, (r0 + r1) / 2.0);
            let dst = snap(
                l + u0 * (r - l) - origin.x,
                a - origin.y,
                l + u1 * (r - l) - origin.x,
                b - origin.y,
            );
            out.extend(piece_element(
                renderer,
                piece,
                Some((y0, y1)),
                dst,
                scale,
                alpha,
            ));
        }
    }
    out
}

impl State {
    /// Whether windows move: not in tests that ask for instant motion.
    fn moving(&self) -> bool {
        !self.instant_motion
    }

    fn reduced(&self) -> bool {
        self.config.appearance.reduced_motion
    }

    /// The window's surfaces as drawn last, if it was drawn.
    pub(crate) fn picture_of(&self, window: &Window) -> Option<Snapshot> {
        let grab = self.snapshots.grab.get()?;
        let surface = window.wl_surface()?;
        let geometry = window.geometry();
        let pieces = grab(&surface, Point::default() - geometry.loc.to_f64());
        (!pieces.is_empty()).then(|| Snapshot {
            pieces,
            size: geometry.size.to_f64(),
        })
    }

    /// The window's geometry, global, where it is shown.
    fn shown_rect(&self, window: &Window) -> Option<Rectangle<f64, Logical>> {
        let at = self.space.element_location(window)?;
        Some(Rectangle::new(at, window.geometry().size).to_f64())
    }

    /// Before the drawing starts: the snapshot taker, once.
    pub fn ready_snapshots<R>(&self, renderer: &R)
    where
        R: Renderer,
        R::TextureId: Texture + Clone + 'static,
    {
        self.snapshots
            .grab
            .get_or_init(|| grabber(renderer.context_id()));
    }

    /// A window's surface going: its picture, while its textures are still
    /// there, in case its toplevel goes after.
    pub fn surface_going(&mut self, surface: &WlSurface) {
        if !self.moving() {
            return;
        }
        let Some((id, window)) = self
            .windows
            .iter()
            .find(|(_, m)| m.window.wl_surface().is_some_and(|s| *s == *surface))
            .map(|(id, m)| (*id, m.window.clone()))
        else {
            return;
        };
        if let Some(picture) = self.picture_of(&window) {
            self.snapshots.last.insert(id, picture);
        }
    }

    /// The windows closing in the Matrix theme whose glitch plays at
    /// `now`, for the GPU to separate their colours (#169).
    pub(crate) fn glitching(&self, now: std::time::Duration) -> Vec<crate::crt::Glitching> {
        self.snapshots
            .closing
            .iter()
            .filter_map(|c| {
                let Fade::Matrix(close) = &c.fade else {
                    return None;
                };
                Some(crate::crt::Glitching {
                    id: c.id,
                    frame: c.frame,
                    picture: c.picture.clone(),
                    bands: close.bands(now)?,
                    split: close.split(now),
                })
            })
            .collect()
    }

    /// A window going: it dissolves from how it is now.
    pub fn window_closing(&mut self, id: WindowId) {
        self.snapshots.away.remove(&id);
        let last = self.snapshots.last.remove(&id);
        if !self.moving() {
            return;
        }
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        let window = managed.window.clone();
        let (Some(picture), Some(frame)) =
            (self.picture_of(&window).or(last), self.shown_rect(&window))
        else {
            return;
        };
        let mut motion = managed.motion;
        let mut opacity = managed.opacity;
        let now = self.clock.now();
        let fade = if self.look() == Look::Matrix && !self.reduced() {
            // From what of it shows, were it still materializing.
            opacity *= motion.at(now).0;
            Fade::Matrix(Box::new(matrix::Close::new(now, frame.size)))
        } else {
            motion.closing(now, self.reduced());
            Fade::Plain(motion)
        };
        let decorated = self.decorated(&window);
        self.snapshots.closing.push(Closing {
            id,
            picture,
            frame,
            decorated,
            opacity,
            fade,
        });
        self.needs_redraw = true;
    }

    /// Where a window minimizes to: its dock icon when the dock shows, the
    /// column otherwise, or the bottom of its screen.
    fn genie_target(&self, id: WindowId, window: &Window) -> Rectangle<f64, Logical> {
        if let Some(icon) = self.dock_icon_of(id) {
            return icon;
        }
        let output = self.output_of(window).cloned();
        if let Some(column) = output.as_ref().and_then(|o| self.column_rect(o)) {
            let c = column.to_f64();
            return Rectangle::new(
                (c.loc.x + c.size.w * 0.2, c.loc.y + c.size.h * 0.4).into(),
                (c.size.w * 0.6, 48.0).into(),
            );
        }
        let area = output
            .and_then(|o| self.space.output_geometry(&o))
            .unwrap_or_default()
            .to_f64();
        Rectangle::new(
            (
                area.loc.x + area.size.w / 2.0 - 32.0,
                area.loc.y + area.size.h - 64.0,
            )
                .into(),
            (64.0, 64.0).into(),
        )
    }

    /// How windows go into the dock and come back now, and how long it
    /// takes.
    fn away(&self) -> (Away, std::time::Duration) {
        if self.reduced() {
            (Away::Fade, SMALL)
        } else if self.look() == Look::Matrix {
            (Away::Crt, SWITCH)
        } else {
            (Away::Lamp, LARGE)
        }
    }

    /// Before a window minimizes: its picture, and where it goes.
    pub fn genie_minimize(&mut self, id: WindowId) {
        if !self.moving() {
            return;
        }
        let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) else {
            return;
        };
        let (Some(picture), Some(rect)) = (self.picture_of(&window), self.shown_rect(&window))
        else {
            return;
        };
        let target = self.genie_target(id, &window);
        let now = self.clock.now();
        let (away, duration) = self.away();
        let mut progress = Animated::new(0.0);
        progress.set(1.0, Curve::Ease(Easing::Linear, duration), now);
        let decorated = self.decorated(&window);
        self.snapshots.genies.retain(|g| g.id != id);
        self.snapshots.away.insert(id, (picture.clone(), target));
        self.snapshots.genies.push(Genie {
            id,
            picture,
            window: rect,
            target,
            progress,
            restoring: false,
            away,
            decorated,
        });
        self.needs_redraw = true;
    }

    /// After the desktops change: minimized windows that came back play
    /// the magic lamp backwards, from where they went.
    pub fn track_minimized(&mut self) {
        let now_minimized: BTreeSet<WindowId> = self
            .windows
            .keys()
            .copied()
            .filter(|id| self.model.is_minimized(*id))
            .collect();
        let back: Vec<WindowId> = self
            .snapshots
            .minimized
            .difference(&now_minimized)
            .copied()
            .collect();
        self.snapshots.minimized = now_minimized;
        for id in back {
            let Some((picture, target)) = self.snapshots.away.remove(&id) else {
                continue;
            };
            let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) else {
                continue;
            };
            let Some(rect) = self.shown_rect(&window) else {
                continue;
            };
            let now = self.clock.now();
            let (away, duration) = self.away();
            // The lamp turns back from where it is; the others come back
            // from the start, as nothing of the window shows on the way.
            let current = self
                .snapshots
                .genies
                .iter()
                .find(|g| g.id == id && away == Away::Lamp)
                .map_or(1.0, |g| g.progress.value(now));
            let mut progress = Animated::new(current);
            progress.set(0.0, Curve::Ease(Easing::Linear, duration), now);
            let decorated = self.decorated(&window);
            self.snapshots.genies.retain(|g| g.id != id);
            self.snapshots.genies.push(Genie {
                id,
                picture,
                window: rect,
                target,
                progress,
                restoring: true,
                away,
                decorated,
            });
            self.needs_redraw = true;
        }
    }

    /// Before a window changes place by maximizing or going fullscreen.
    pub fn morph_from_here(&mut self, id: WindowId) {
        let change = if self.reduced() {
            Change::Fade
        } else if self.look() == Look::Matrix {
            Change::Matrix(Shift::Maximize)
        } else {
            Change::Morph
        };
        self.change_from_here(id, change);
    }

    /// Before a window snaps: in the Matrix theme it springs into place;
    /// elsewhere it is there at once.
    pub fn snap_from_here(&mut self, id: WindowId) {
        if self.matrix_moves() {
            self.change_from_here(id, Change::Matrix(Shift::Snap));
        }
    }

    fn change_from_here(&mut self, id: WindowId, change: Change) {
        if !self.moving() {
            return;
        }
        let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) else {
            return;
        };
        let (Some(picture), Some(from)) = (self.picture_of(&window), self.shown_rect(&window))
        else {
            return;
        };
        let now = self.clock.now();
        let from = self
            .snapshots
            .morphs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| Some(m.view(self.shown_rect(&window)?, now).window))
            .unwrap_or(from);
        let curve = match change {
            Change::Morph => Curve::Ease(Easing::OutCubic, STANDARD),
            Change::Matrix(shift) => Curve::Ease(Easing::Linear, shift.duration()),
            Change::Fade => Curve::Ease(Easing::Linear, SMALL),
        };
        let mut progress = Animated::new(0.0);
        progress.set(1.0, curve, now);
        self.snapshots.morphs.retain(|m| m.id != id);
        self.snapshots.morphs.push(Morph {
            id,
            picture,
            from,
            to: None,
            progress,
            change,
            seed: hash(&[now.as_nanos() as u64, id.0]),
        });
        self.needs_redraw = true;
    }

    /// Where a morphing window was asked to go, global.
    pub fn morph_to(&mut self, id: WindowId, rect: Rectangle<i32, Logical>) {
        if let Some(m) = self.snapshots.morphs.iter_mut().find(|m| m.id == id) {
            m.to = Some(rect.to_f64());
        }
    }

    /// A mode change that moved nothing.
    pub fn snapshots_unmorph(&mut self, id: WindowId) {
        self.snapshots.morphs.retain(|m| m.id != id);
    }

    /// Whether a window is hidden behind its own magic lamp, coming back.
    pub fn genie_hides(&self, id: WindowId) -> bool {
        let now = self.clock.now();
        self.snapshots
            .genies
            .iter()
            .any(|g| g.id == id && g.restoring && g.progress.is_running(now))
    }

    /// Where a window changing place is drawn now, and its old picture;
    /// `live` is where it is shown, global.
    pub fn morph_view(&self, id: WindowId, live: Rectangle<f64, Logical>) -> Option<MorphView> {
        let now = self.clock.now();
        let m = self
            .snapshots
            .morphs
            .iter()
            .find(|m| m.id == id && m.progress.is_running(now))?;
        Some(m.view(live, now))
    }

    /// The morphing window's old picture over it, fading.
    pub fn morph_picture<R>(
        &self,
        renderer: &R,
        id: WindowId,
        into: Rectangle<f64, Logical>,
        alpha: f32,
        scale: f64,
    ) -> Vec<TextureRenderElement<R::TextureId>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        self.snapshots
            .morphs
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.picture.stretched(renderer, into, scale, alpha))
            .unwrap_or_default()
    }

    /// Whether a picture still moves at `now`.
    pub fn snapshots_moving(&self, now: std::time::Duration) -> bool {
        let s = &self.snapshots;
        s.closing.iter().any(|c| c.is_running(now))
            || s.genies.iter().any(|g| g.progress.is_running(now))
            || s.morphs.iter().any(|m| m.progress.is_running(now))
    }

    /// Pictures done moving go.
    pub fn settle_snapshots(&mut self) {
        let now = self.clock.now();
        let s = &mut self.snapshots;
        s.closing.retain(|c| c.is_running(now));
        s.genies.retain(|g| g.progress.is_running(now));
        s.morphs.retain(|m| m.progress.is_running(now));
        let windows = &self.windows;
        s.away.retain(|id, _| windows.contains_key(id));
        s.last.retain(|id, _| windows.contains_key(id));
    }

    /// Windows dissolving and pouring into the dock, over the others.
    pub fn snapshot_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let origin = area.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let now = self.clock.now();
        let mut out = Vec::new();
        for genie in &self.snapshots.genies {
            let p = f64::from(genie.progress.value(now));
            let bounds = genie.window.merge(genie.target);
            if !bounds.overlaps(area.to_f64()) {
                continue;
            }
            let radius = if genie.decorated {
                self.style.radius
            } else {
                0.0
            };
            match genie.away {
                Away::Lamp => {
                    // It thins out as it reaches the icon.
                    let alpha = 1.0 - smoothstep((p - 0.75) / 0.25) as f32;
                    out.extend(
                        genie
                            .elements(renderer, p, origin, scale, alpha)
                            .into_iter()
                            .map(|e| SceneElement::Snapshot(Rounded::new(e, unclipped(), 0))),
                    );
                }
                Away::Crt => out.extend(self.crt(renderer, genie, Layer::Picture, origin, scale)),
                Away::Fade => {
                    let local = Rectangle::new(genie.window.loc - origin, genie.window.size);
                    let clip = local.to_physical_precise_round(scale);
                    let radius = (radius * scale).round() as i32;
                    out.extend(
                        genie
                            .picture
                            .stretched(renderer, local, scale, 1.0 - p as f32)
                            .into_iter()
                            .map(|e| SceneElement::Snapshot(Rounded::new(e, clip, radius))),
                    );
                }
            }
        }
        for closing in self.snapshots.closing.iter().rev() {
            if !closing.frame.overlaps(area.to_f64()) {
                continue;
            }
            let motion = match &closing.fade {
                Fade::Plain(motion) => motion,
                Fade::Matrix(close) => {
                    out.extend(
                        self.matrix_closing(renderer, output, closing, close, origin, scale),
                    );
                    continue;
                }
            };
            let (alpha, grow) = motion.at(now);
            let grow = f64::from(grow);
            let middle = closing.frame.loc + closing.frame.size.to_point().downscale(2.0);
            let local = Rectangle::new(
                middle - closing.frame.size.to_point().upscale(grow / 2.0) - origin,
                closing.frame.size.upscale(grow),
            );
            let radius = if closing.decorated {
                (self.style.radius * scale * grow).round() as i32
            } else {
                0
            };
            let clip = local.to_physical_precise_round(scale);
            if closing.decorated {
                out.extend(
                    hairline_elements(
                        renderer,
                        &self.textures,
                        &self.style,
                        local,
                        scale,
                        alpha * closing.opacity,
                    )
                    .into_iter()
                    .map(SceneElement::Frame),
                );
            }
            out.extend(
                closing
                    .picture
                    .stretched(renderer, local, scale, alpha * closing.opacity)
                    .into_iter()
                    .map(|e| SceneElement::Snapshot(Rounded::new(e, clip, radius))),
            );
            if closing.decorated {
                out.extend(
                    shadow_elements(renderer, &self.textures, &self.style, local, scale, alpha)
                        .into_iter()
                        .map(SceneElement::Shadow),
                );
            }
        }
        out
    }
}

impl State {
    /// A window going into the dock or coming out in the Matrix theme.
    fn crt<R>(
        &self,
        renderer: &mut R,
        genie: &Genie,
        layer: Layer,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let switch = Switch {
            picture: &genie.picture,
            window: genie.window,
            target: genie.target,
            radius: if genie.decorated {
                self.style.radius
            } else {
                0.0
            },
        };
        // Restoring runs its own course as the progress goes back to 0.
        let p = genie.progress.value(self.clock.now());
        let along = if genie.restoring { 1.0 - p } else { p };
        let t = along * SWITCH.as_secs_f32() * 1000.0;
        self.matrix_switch_elements(
            renderer,
            &switch,
            (t, genie.restoring),
            layer,
            origin,
            scale,
        )
    }

    /// What shines over the panels as windows change place in the Matrix
    /// theme: the CRT's line and flash at the dock, and the circuit traces
    /// of windows maximizing. Front first.
    pub fn snapshot_lights<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let (origin, area) = (area.loc.to_f64(), area.to_f64());
        let scale = output.current_scale().fractional_scale();
        let now = self.clock.now();
        let mut out = Vec::new();
        for genie in &self.snapshots.genies {
            if genie.away == Away::Crt && genie.window.merge(genie.target).overlaps(area) {
                out.extend(self.crt(renderer, genie, Layer::Light, origin, scale));
            }
        }
        for m in &self.snapshots.morphs {
            let Change::Matrix(Shift::Maximize) = m.change else {
                continue;
            };
            let live = self
                .windows
                .get(&m.id)
                .and_then(|w| self.shown_rect(&w.window));
            let Some(to) = m.to.or(live) else {
                continue;
            };
            if !m.progress.is_running(now) || !m.from.merge(to).overlaps(area) {
                continue;
            }
            let p = m.progress.value(now);
            out.extend(self.matrix_shift_elements(
                renderer,
                (m.from, to),
                p,
                m.seed,
                origin,
                scale,
            ));
        }
        out
    }

    /// A closing window in the Matrix theme: its picture, glitching, under
    /// the glyphs that take it, then the rain it drains off as.
    fn matrix_closing<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        closing: &Closing,
        close: &matrix::Close,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let now = self.clock.now();
        let local = Rectangle::new(closing.frame.loc - origin, closing.frame.size);
        let mut out = self.matrix_close_elements(renderer, close, local, scale);
        let alpha = close.picture_alpha(now) * closing.opacity;
        if alpha > 0.0 {
            let radius = if closing.decorated {
                (self.style.radius * scale).round() as i32
            } else {
                0
            };
            let clip = local.to_physical_precise_round(scale);
            let bands = close.bands(now);
            // On the GPU, its colours apart too (#169).
            let glitched = bands.as_ref().and_then(|_| {
                self.glitch_element(
                    renderer,
                    output,
                    closing.id,
                    clip,
                    local.size.to_i32_round(),
                    alpha,
                )
            });
            let picture = match (glitched, bands) {
                (Some(glitched), _) => vec![glitched],
                (None, Some(bands)) => bands
                    .into_iter()
                    .flat_map(|(v0, v1, dx)| {
                        closing
                            .picture
                            .band(renderer, local, (v0, v1), dx, scale, alpha)
                    })
                    .collect(),
                (None, None) => closing.picture.stretched(renderer, local, scale, alpha),
            };
            if closing.decorated {
                out.extend(
                    hairline_elements(renderer, &self.textures, &self.style, local, scale, alpha)
                        .into_iter()
                        .map(SceneElement::Frame),
                );
            }
            out.extend(
                picture
                    .into_iter()
                    .map(|e| SceneElement::Snapshot(Rounded::new(e, clip, radius))),
            );
        }
        if closing.decorated {
            let alpha = close.shadow_alpha(now) * closing.opacity;
            out.extend(
                shadow_elements(renderer, &self.textures, &self.style, local, scale, alpha)
                    .into_iter()
                    .map(SceneElement::Shadow),
            );
        }
        out
    }
}

/// No clip: the picture as it is.
pub(crate) fn unclipped() -> Rectangle<i32, smithay::utils::Physical> {
    Rectangle::new(
        (i32::MIN / 4, i32::MIN / 4).into(),
        (i32::MAX / 2, i32::MAX / 2).into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn the_lamp_starts_as_the_window_and_ends_in_the_icon() {
        let window = rect(100.0, 100.0, 800.0, 500.0);
        let icon = rect(400.0, 900.0, 68.0, 68.0);
        for v in [0.0, 0.5, 1.0] {
            let (y, l, r) = genie_row(window, icon, v, 0.0);
            assert!((y - (100.0 + v * 500.0)).abs() < 1e-9);
            assert!((l - 100.0).abs() < 1e-9 && (r - 900.0).abs() < 1e-9);
            let (y, l, r) = genie_row(window, icon, v, 1.0);
            assert!((y - (900.0 + v * 68.0)).abs() < 1e-9, "{v}: {y}");
            assert!(
                (l - 400.0).abs() < 1e-6 && (r - 468.0).abs() < 1e-6,
                "{l} {r}"
            );
        }
    }

    #[test]
    fn halfway_the_bottom_is_narrower_and_ahead() {
        let window = rect(100.0, 100.0, 800.0, 500.0);
        let icon = rect(400.0, 900.0, 68.0, 68.0);
        let (top_y, top_l, top_r) = genie_row(window, icon, 0.0, 0.5);
        let (bottom_y, bottom_l, bottom_r) = genie_row(window, icon, 1.0, 0.5);
        assert!(bottom_r - bottom_l < top_r - top_l, "a funnel");
        assert!(bottom_y > top_y, "rows keep their order");
        assert!(
            (bottom_y - 600.0) / 368.0 > (top_y - 100.0) / 800.0,
            "the far end is further along its way"
        );
    }

    #[test]
    fn going_up_is_the_same_upside_down() {
        let window = rect(100.0, 400.0, 800.0, 500.0);
        let icon = rect(400.0, 0.0, 68.0, 68.0);
        let (y, l, r) = genie_row(window, icon, 0.0, 1.0);
        assert!((y - 0.0).abs() < 1e-6, "{y}");
        assert!((l - 400.0).abs() < 1e-6 && (r - 468.0).abs() < 1e-6);
        let (top, _, _) = genie_row(window, icon, 0.0, 0.5);
        let (bottom, _, _) = genie_row(window, icon, 1.0, 0.5);
        assert!(top < bottom);
    }
}
