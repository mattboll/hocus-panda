//! What an output shows, front to back: the cursor, then each window from
//! the top with its focus ring, its content with rounded corners, and its
//! shadow. The headless and nested backends draw it, and screenshots too.

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, memory::MemoryRenderBufferRenderElement, render_elements,
            surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::Window,
    output::Output,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
    utils::{Logical, Point, Rectangle, Scale},
};

use crate::{
    State,
    cursor::CursorElement,
    decoration::{Rounded, ring_elements, shadow_elements},
    desktops::model_id,
    window_mode::WindowMode,
};

render_elements! {
    pub SceneElement<R> where R: ImportAll + ImportMem;
    Cursor=CursorElement<R>,
    Window=Rounded<WaylandSurfaceRenderElement<R>>,
    Frame=MemoryRenderBufferRenderElement<R>,
}

impl State {
    /// Everything `output` shows, front first.
    pub fn scene_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        cursor: bool,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let mut elements = Vec::new();
        if cursor {
            elements.extend(
                self.cursor_elements(renderer, output)
                    .into_iter()
                    .map(SceneElement::Cursor),
            );
        }
        let Some(area) = self.space.output_geometry(output) else {
            return elements;
        };
        let scale = output.current_scale().fractional_scale();
        let focused = self.focused_window();
        let windows: Vec<&Window> = self.space.elements().collect();
        for window in windows.into_iter().rev() {
            let Some(location) = self.space.element_location(window) else {
                continue;
            };
            let geometry = window.geometry();
            let rect = Rectangle::<i32, Logical>::new(location - area.loc, geometry.size);
            let bbox = Rectangle::new(
                location - geometry.loc + window.bbox().loc,
                window.bbox().size,
            );
            if !bbox.overlaps(area) && !rect.overlaps(Rectangle::from_size(area.size)) {
                continue;
            }
            let decorated = self.decorated(window);
            if decorated && focused.as_ref() == Some(window) {
                let ring =
                    ring_elements(renderer, &self.textures, &self.style, rect.to_f64(), scale);
                elements.extend(ring.into_iter().map(SceneElement::Frame));
            }
            let radius = if decorated {
                (self.style.radius * scale).round() as i32
            } else {
                0
            };
            let clip = rect.to_f64().to_physical_precise_round(scale);
            let origin: Point<i32, _> =
                (location - geometry.loc - area.loc).to_physical_precise_round(scale);
            let content: Vec<WaylandSurfaceRenderElement<R>> =
                window.render_elements(renderer, origin, Scale::from(scale), 1.0);
            elements.extend(
                content
                    .into_iter()
                    .map(|e| SceneElement::Window(Rounded::new(e, clip, radius))),
            );
            if decorated {
                let shadow =
                    shadow_elements(renderer, &self.textures, &self.style, rect.to_f64(), scale);
                elements.extend(shadow.into_iter().map(SceneElement::Frame));
            }
        }
        elements
    }

    /// Whether the compositor draws the window's corners, shadow and ring:
    /// in normal mode, when the window has no shadow of its own (server-side
    /// decorations, or a buffer no larger than the window).
    fn decorated(&self, window: &Window) -> bool {
        let normal = model_id(window)
            .and_then(|id| self.windows.get(&id))
            .is_none_or(|m| m.mode == WindowMode::Normal);
        let server_side = window
            .toplevel()
            .is_some_and(|t| t.current_state().decoration_mode == Some(DecorationMode::ServerSide));
        let own_shadow = window.bbox().size != window.geometry().size;
        normal && (server_side || !own_shadow)
    }
}
