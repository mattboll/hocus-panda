//! What an output shows, front to back: the cursor, then each window from
//! the top with its focus ring, its content with rounded corners, and its
//! shadow. The headless and nested backends draw it, and screenshots too.

use hocus_core::Zone;
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Kind, memory::MemoryRenderBufferRenderElement, render_elements,
            solid::SolidColorRenderElement, surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::Window,
    output::Output,
    utils::{Logical, Point, Rectangle, Scale},
};

use crate::{
    State,
    cursor::CursorElement,
    decoration::{Rounded, ring_elements, shadow_elements},
    desktops::{model_id, rect},
    window_mode::WindowMode,
};

render_elements! {
    pub SceneElement<R> where R: ImportAll + ImportMem;
    Cursor=CursorElement<R>,
    Window=Rounded<WaylandSurfaceRenderElement<R>>,
    Frame=MemoryRenderBufferRenderElement<R>,
    Solid=SolidColorRenderElement,
}

impl State {
    /// Shows, moves or hides the zone a dragged window would snap to.
    pub fn set_snap_preview(&mut self, preview: Option<(Output, Zone)>) {
        if self.snap_preview == preview {
            return;
        }
        if let Some((output, zone)) = &preview {
            let target = zone.rect(rect(self.work_area(output)), self.config.windows.gaps.inner);
            let [r, g, b] = self.style.accent;
            // The accent, see-through, premultiplied.
            let a = 0.22;
            let c = |v: u8| f32::from(v) / 255.0 * a;
            self.snap_preview_buffer
                .update((target.width, target.height), [c(r), c(g), c(b), a]);
        }
        self.snap_preview = preview;
        self.needs_redraw = true;
    }

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
        if let Some((preview_output, zone)) = &self.snap_preview
            && preview_output == output
        {
            let target = zone.rect(rect(self.work_area(output)), self.config.windows.gaps.inner);
            let local = Rectangle::<i32, Logical>::new(
                Point::from((target.x, target.y)) - area.loc,
                (target.width, target.height).into(),
            );
            elements.extend(
                ring_elements(renderer, &self.textures, &self.style, local.to_f64(), scale)
                    .into_iter()
                    .map(SceneElement::Frame),
            );
            elements.push(SceneElement::Solid(SolidColorRenderElement::from_buffer(
                &self.snap_preview_buffer,
                local.loc.to_physical_precise_round(scale),
                scale,
                1.0,
                Kind::Unspecified,
            )));
        }
        // Toasts, then the column, above windows; the column under a
        // fullscreen one.
        elements.extend(
            self.toast_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        elements.extend(
            self.shell_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
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
            let opacity = model_id(window)
                .and_then(|id| self.windows.get(&id))
                .map_or(1.0, |m| m.opacity);
            let content: Vec<WaylandSurfaceRenderElement<R>> =
                window.render_elements(renderer, origin, Scale::from(scale), opacity);
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
    /// in normal or snapped mode, when the window has no shadow of its own (server-side
    /// decorations, or a buffer no larger than the window).
    fn decorated(&self, window: &Window) -> bool {
        let normal = model_id(window)
            .and_then(|id| self.windows.get(&id))
            .is_none_or(|m| {
                matches!(
                    m.mode,
                    WindowMode::Normal | WindowMode::Snapped { .. } | WindowMode::Tiled { .. }
                )
            });
        let server_side = crate::surface::server_side(window);
        let own_shadow = window.bbox().size != window.geometry().size;
        normal && (server_side || !own_shadow)
    }
}
