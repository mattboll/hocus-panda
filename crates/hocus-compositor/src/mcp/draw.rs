//! An MCP agent's pointer and the ring before it acts (docs/design/mcp.md,
//! scene 1), drawn by the compositor: the shell's surfaces have no alpha
//! when they draw on the CPU, so an arrow's shape and a ring cannot be
//! theirs. The mockup's arrow (its SVG path, violet rimmed in the dark
//! ink) and its ring (34 px, 2 px of violet, a 6 px halo), drawn on the
//! CPU once per scale and kept. On the screen only: never in a capture,
//! a share or a recording. The agent's name and intent beside them are
//! the shell's tag ([`hocus_link::McpAt::Pointer`]).

use std::cell::RefCell;

use resvg::{tiny_skia, usvg};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, memory::MemoryRenderBuffer},
    },
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    State,
    decoration::{Keyed, Placed, buffer},
    render::{OutputElement, OutputElements},
};

/// The arrow's view box, and where its tip is in it.
const ARROW: (f64, f64) = (20.0, 24.0);
const TIP: (f64, f64) = (2.0, 2.0);
const ARROW_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="24" viewBox="0 0 20 24"><path d="M2 2 L2 19 L6.5 15 L9.5 22 L12.5 20.7 L9.6 13.8 L16 13.8 Z" fill="#B69CFF" stroke="#1b1230" stroke-width="1.6" stroke-linejoin="round"/></svg>"##;
/// The ring's side, its halo outside it.
const RING: f64 = 34.0;
const HALO: f64 = 6.0;
const RING_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="46" height="46" viewBox="0 0 46 46"><circle cx="23" cy="23" r="20" fill="none" stroke="#B69CFF" stroke-opacity="0.30" stroke-width="6"/><circle cx="23" cy="23" r="16" fill="none" stroke="#B69CFF" stroke-width="2"/></svg>"##;

/// A picture drawn for one scale.
type Picture = RefCell<Option<(u64, MemoryRenderBuffer, i32, i32, Id)>>;

/// The pictures, kept.
#[derive(Debug, Default)]
pub struct Pictures {
    arrow: Picture,
    ring: Picture,
}

/// `svg` drawn `side` logical pixels wide at `scale`: its pixels, ARGB.
fn draw(svg: &str, size: (f64, f64), scale: f64) -> Option<(Vec<u8>, i32, i32)> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let (w, h) = (
        (size.0 * scale).ceil() as u32,
        (size.1 * scale).ceil() as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let k = scale as f32;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    Some((
        crate::radial_draw::argb_of(pixmap.take()),
        w as i32,
        h as i32,
    ))
}

impl State {
    /// The agents' pointers on `output`, and the ring of the one about to
    /// act, front first.
    pub(crate) fn mcp_pointer_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if self.locked() {
            return;
        }
        let Some(area) = self.space.output_geometry(output) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let key = (scale * 1000.0) as u64;
        let pictures = &self.mcp.pictures;
        let mut place = |picture: &Picture,
                         svg: &str,
                         size: (f64, f64),
                         corner: Point<f64, Logical>,
                         out: &mut Vec<OutputElement<R>>| {
            let mut cache = picture.borrow_mut();
            if cache.as_ref().is_none_or(|(k, ..)| *k != key)
                && let Some((pixels, w, h)) = draw(svg, size, scale)
            {
                *cache = Some((key, buffer(&pixels, w, h), w, h, Id::new()));
            }
            let Some((_, texture, w, h, id)) = cache.as_ref() else {
                return;
            };
            let dst = Rectangle::<f64, Logical>::new(corner, size.into())
                .to_physical_precise_round::<f64, i32>(scale);
            let all = Rectangle::from_size((f64::from(*w), f64::from(*h)).into());
            match Placed::new(renderer, texture, all, dst, 1.0) {
                Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, id.clone()))),
                Err(err) => tracing::warn!(?err, "an agent's pointer is not drawn"),
            }
        };
        for (at, ring) in self.mcp_pointers_on(output) {
            let local = at - area.loc.to_f64();
            place(
                &pictures.arrow,
                ARROW_SVG,
                ARROW,
                local - Point::from(TIP),
                out,
            );
            if ring {
                let side = RING + 2.0 * HALO;
                place(
                    &pictures.ring,
                    RING_SVG,
                    (side, side),
                    local - Point::from((side / 2.0, side / 2.0)),
                    out,
                );
            }
        }
    }
}
