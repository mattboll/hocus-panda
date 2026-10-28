//! Pictures of a window that outlive it (docs/design/windows.md §5, §10):
//! a closing window fades from its last frame without waiting for its
//! application, and a minimised one shrinks away. The surfaces' textures,
//! already on the graphics card, are kept by handle (nothing is copied)
//! and drawn stretched as the motion goes. smithay drops a surface's
//! textures with the surface, so they are taken as it goes, before.
//! Ported from the first build's `snapshot.rs`, without its other motions.

use std::{any::Any, cell::OnceCell, rc::Rc};

use smithay::{
    backend::renderer::{
        ContextId, Renderer, Texture,
        element::{Id, Kind, texture::TextureRenderElement},
        utils::RendererSurfaceStateUserData,
    },
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, Size, Transform},
    wayland::compositor::{SurfaceData, TraversalAction, with_states, with_surface_tree_downward},
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
    /// An id per piece, the same over the motion.
    id: Id,
}

/// A window's surfaces as they were drawn, back first.
#[derive(Clone)]
pub struct Snapshot {
    pieces: Vec<Piece>,
    /// The window's geometry size.
    pub size: Size<f64, Logical>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("pieces", &self.pieces.len())
            .field("size", &self.size)
            .finish()
    }
}

type Grab = Box<dyn Fn(&WlSurface, Point<f64, Logical>) -> Vec<Piece>>;

/// Takes a surface tree's textures: made by the first drawing, which knows
/// the renderer's texture type.
#[derive(Default)]
pub struct Grabber(OnceCell<Grab>);

impl std::fmt::Debug for Grabber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grabber")
            .field("ready", &self.0.get().is_some())
            .finish()
    }
}

impl Grabber {
    /// Before the drawing starts: the taker, once.
    pub fn ready<R>(&self, renderer: &R)
    where
        R: Renderer,
        R::TextureId: Texture + Clone + 'static,
    {
        self.0.get_or_init(|| grabber(renderer.context_id()));
    }

    /// The window's surfaces as drawn last, `geometry_loc` its geometry's
    /// corner in its surface, `size` its geometry's size.
    pub fn take(
        &self,
        surface: &WlSurface,
        geometry_loc: Point<i32, Logical>,
        size: Size<i32, Logical>,
    ) -> Option<Snapshot> {
        let grab = self.0.get()?;
        let pieces = grab(surface, Point::default() - geometry_loc.to_f64());
        (!pieces.is_empty()).then(|| Snapshot {
            pieces,
            size: size.to_f64(),
        })
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
                id: Id::new(),
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

/// Whole-number edges, so neighbouring parts meet.
fn snap(x0: f64, y0: f64, x1: f64, y1: f64) -> Rectangle<i32, Logical> {
    let (x0, y0) = (x0.round() as i32, y0.round() as i32);
    let (x1, y1) = (x1.round() as i32, y1.round() as i32);
    Rectangle::new((x0, y0).into(), ((x1 - x0).max(0), (y1 - y0).max(0)).into())
}

impl Snapshot {
    /// Its pieces stretched into `into` (logical, output-local), front
    /// first.
    pub fn stretched<R>(
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
                if dst.size.w <= 0 || dst.size.h <= 0 {
                    return None;
                }
                let texture = p.texture.downcast_ref::<R::TextureId>()?.clone();
                Some(TextureRenderElement::from_static_texture(
                    p.id.clone(),
                    renderer.context_id(),
                    dst.loc.to_f64().to_physical(scale),
                    texture,
                    p.scale,
                    p.transform,
                    Some(alpha),
                    Some(p.src),
                    Some(dst.size),
                    None,
                    Kind::Unspecified,
                ))
            })
            .collect()
    }
}
