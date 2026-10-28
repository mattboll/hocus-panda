//! What an output shows, front first: the cursor, then the layer surfaces
//! above the windows, the windows (popups included), and the layers under
//! them; and what clients are told once a frame is out.

use std::time::Duration;

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            RenderElementStates, default_primary_scanout_output_compare, render_elements,
            surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::{
        layer_map_for_output,
        space::{SpaceRenderElements, space_render_elements},
        utils::{
            OutputPresentationFeedback, send_frames_surface_tree, surface_primary_scanout_output,
            update_surface_primary_scanout_output,
        },
    },
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::Monotonic,
    wayland::{
        compositor::{SurfaceData, send_surface_state},
        fractional_scale::with_fractional_scale,
        presentation::Refresh,
    },
};

use crate::{State, cursor::CursorElement};

/// The background, until wallpapers: the dark theme's base colour.
pub const BACKGROUND: [f32; 4] = [0.055, 0.059, 0.071, 1.0];

render_elements! {
    pub OutputElements<R, E> where R: ImportAll + ImportMem;
    Cursor = CursorElement<R>,
    Space = SpaceRenderElements<R, E>,
}

/// What an output shows, element by element.
pub type OutputElement<R> = OutputElements<R, WaylandSurfaceRenderElement<R>>;

/// Tells a surface the scale and transform of the output it shows most on:
/// fractional-scale's, and wl_surface's preferred buffer scale for clients
/// that only draw at whole scales.
fn tell_scale(surface: &WlSurface, data: &SurfaceData, output: Option<&Output>) {
    let Some(output) = output else {
        return;
    };
    let scale = output.current_scale();
    with_fractional_scale(data, |fractional| {
        fractional.set_preferred_scale(scale.fractional_scale());
    });
    send_surface_state(
        surface,
        data,
        scale.integer_scale(),
        output.current_transform(),
    );
}

impl State {
    /// What `output` shows, front first. `cursor`: whether the compositor
    /// draws the cursor (nested, the host does).
    pub fn output_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        cursor: bool,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let mut elements: Vec<OutputElement<R>> = Vec::new();
        if cursor {
            elements.extend(
                self.cursor_elements(renderer, output)
                    .into_iter()
                    .map(OutputElements::Cursor),
            );
        }
        match space_render_elements(renderer, [&self.space], output, 1.0) {
            Ok(space) => elements.extend(space.into_iter().map(OutputElements::Space)),
            Err(err) => tracing::warn!(?err, output = output.name(), "nothing to draw"),
        }
        elements
    }

    /// Tells the clients shown on `output` that a frame went out, so that
    /// they draw their next one. `states` says which surfaces it showed,
    /// when the backend knows: a surface is then told only by the output
    /// it shows most on.
    pub fn send_frames(&self, output: &Output, states: Option<&RenderElementStates>) {
        let now: Duration = self.clock.now().into();
        // Frames are paced by the backend, never throttled here.
        let throttle = Some(Duration::ZERO);
        let primary = |surface: &_, data: &_| -> Option<Output> {
            match states {
                Some(_) => surface_primary_scanout_output(surface, data),
                None => Some(output.clone()),
            }
        };
        for window in self.space.elements() {
            window.with_surfaces(|surface, data| {
                if let Some(states) = states {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        data,
                        states,
                        default_primary_scanout_output_compare,
                    );
                }
                tell_scale(surface, data, primary(surface, data).as_ref());
            });
            window.send_frame(output, now, throttle, primary);
        }
        for layer in layer_map_for_output(output).layers() {
            layer.with_surfaces(|surface, data| tell_scale(surface, data, Some(output)));
            layer.send_frame(output, now, throttle, |_, _| Some(output.clone()));
        }
        if let CursorImageStatus::Surface(cursor) = &self.cursor {
            send_frames_surface_tree(cursor, output, now, throttle, |_, _| Some(output.clone()));
        }
    }

    /// Tells the windows on `output` that the frame drawn at `time` is on
    /// screen (presentation-time), the `seq`th of the output.
    pub fn presented(&self, output: &Output, time: Duration, seq: u64) {
        use wp_presentation_feedback::Kind;
        let mut feedback = OutputPresentationFeedback::new(output);
        for window in self.space.elements() {
            if self.space.outputs_for_element(window).contains(output) {
                window.take_presentation_feedback(
                    &mut feedback,
                    |_, _| Some(output.clone()),
                    |_, _| Kind::Vsync,
                );
            }
        }
        for layer in layer_map_for_output(output).layers() {
            layer.take_presentation_feedback(
                &mut feedback,
                |_, _| Some(output.clone()),
                |_, _| Kind::Vsync,
            );
        }
        let refresh = output
            .current_mode()
            .filter(|m| m.refresh > 0)
            .map_or(Refresh::Unknown, |m| {
                Refresh::Fixed(Duration::from_secs_f64(1000.0 / f64::from(m.refresh)))
            });
        feedback.presented::<_, Monotonic>(time, refresh, seq, Kind::Vsync);
    }
}
