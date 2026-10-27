//! wlr-layer-shell (#113): third-party bars, launchers, notification
//! daemons and wallpaper tools. Their surfaces sit in four layers among
//! the shell's own: background and bottom under the windows, top above
//! them (under a fullscreen window), overlay above everything but the
//! cursor. Exclusive zones come off the work area, so maximized, snapped
//! and tiled windows make room. A surface may take the keyboard, always
//! (exclusive) or when clicked (on demand).

use smithay::reexports::wayland_server::Resource;
use smithay::{
    backend::renderer::{
        ImportAll, Renderer, Texture,
        element::{AsRenderElements, surface::WaylandSurfaceRenderElement},
    },
    delegate_layer_shell,
    desktop::{LayerSurface, PopupKind, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Scale},
    wayland::{
        compositor::with_states,
        shell::{
            wlr_layer::{
                KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData,
                WlrLayerShellHandler, WlrLayerShellState,
            },
            xdg::PopupSurface,
        },
    },
};

use crate::State;

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.protocols.layer_shell
    }

    /// On the output asked for, or the one under the pointer.
    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.output_under_pointer());
        let Some(output) = output else {
            tracing::warn!(namespace, "a layer surface with no output");
            surface.send_close();
            return;
        };
        let layer = LayerSurface::new(surface, namespace);
        if let Err(err) = layer_map_for_output(&output).map_layer(&layer) {
            tracing::warn!(%err, "could not place a layer surface");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in outputs {
            let mut map = layer_map_for_output(&output);
            let gone = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = gone {
                map.unmap_layer(&layer);
                drop(map);
                self.layers_changed(&output);
            }
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(popup)) {
            tracing::warn!(%err, "could not track a layer popup");
        }
    }
}
delegate_layer_shell!(State);

pub use smithay::wayland::shell::wlr_layer::Layer as LayerKind;

/// The layers drawn above the windows, and those under them.
pub const ABOVE: [Layer; 2] = [Layer::Overlay, Layer::Top];
pub const BELOW: [Layer; 2] = [Layer::Bottom, Layer::Background];

impl State {
    /// A commit of a layer surface: placed again, configured the first
    /// time. Whether `surface` was one.
    pub fn layer_commit(&mut self, surface: &WlSurface) -> bool {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in outputs {
            let mut map = layer_map_for_output(&output);
            let Some(layer) = map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .cloned()
            else {
                continue;
            };
            map.arrange();
            drop(map);
            let configured = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .and_then(|d| d.lock().ok().map(|d| d.initial_configure_sent))
                    .unwrap_or(true)
            });
            if !configured {
                layer.layer_surface().send_configure();
            }
            self.layers_changed(&output);
            return true;
        }
        false
    }

    /// The layers of `output` changed: the work area may have, and a
    /// surface may want the keyboard.
    fn layers_changed(&mut self, output: &Output) {
        let zone = layer_map_for_output(output).non_exclusive_zone();
        let previous = self.protocols.zones.insert(output.name(), zone);
        // Its work area changed: windows move, the shell follows.
        let moved = previous != Some(zone);
        if moved {
            self.relayout();
            self.layout_tiles();
        }
        self.layer_keyboard();
        if moved {
            self.redraw.all();
        } else {
            self.redraw.output(output);
        }
    }

    /// What layer surfaces leave of `output`, global coordinates.
    pub fn non_exclusive_zone(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let geometry = self.space.output_geometry(output)?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(Rectangle::new(geometry.loc + zone.loc, zone.size))
    }

    /// A surface of the top or overlay layer asking for the keyboard
    /// always gets it; when it goes, the windows get it back.
    pub fn layer_keyboard(&mut self) {
        let exclusive = self.exclusive_layer();
        let serial = SERIAL_COUNTER.next_serial();
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        match exclusive {
            Some(layer) => {
                let surface = layer.wl_surface().clone();
                self.protocols.layer_focus = true;
                if keyboard.current_focus().as_ref() != Some(&surface) {
                    keyboard.set_focus(self, Some(surface), serial);
                }
            }
            // An on-demand surface still placed keeps it; one that went
            // gives it back to the windows.
            None if self.protocols.layer_focus
                && !keyboard
                    .current_focus()
                    .is_some_and(|s| s.is_alive() && self.is_layer_surface(&s)) =>
            {
                self.protocols.layer_focus = false;
                self.apply_focus(serial);
            }
            None => {}
        }
    }

    fn is_layer_surface(&self, surface: &WlSurface) -> bool {
        self.space.outputs().any(|o| {
            layer_map_for_output(o)
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_some()
        })
    }

    /// The top or overlay surface that wants the keyboard always.
    fn exclusive_layer(&self) -> Option<LayerSurface> {
        self.space.outputs().find_map(|output| {
            let map = layer_map_for_output(output);
            ABOVE
                .iter()
                .flat_map(|l| map.layers_on(*l).rev().cloned().collect::<Vec<_>>())
                .find(|l| {
                    l.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive
                })
        })
    }

    /// The layer surface under `pos` in `layers` (front first), and where
    /// the surface found is.
    pub fn layer_surface_under(
        &self,
        pos: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(WlSurface, Point<f64, Logical>, LayerSurface)> {
        let output = self.space.outputs().find(|o| {
            self.space
                .output_geometry(o)
                .is_some_and(|g| g.to_f64().contains(pos))
        })?;
        let origin = self.space.output_geometry(output)?.loc;
        let map = layer_map_for_output(output);
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let Some(geometry) = map.layer_geometry(surface) else {
                    continue;
                };
                let at = (origin + geometry.loc).to_f64();
                if let Some((s, p)) = surface.surface_under(pos - at, WindowSurfaceType::ALL) {
                    return Some((s, (p + geometry.loc + origin).to_f64(), surface.clone()));
                }
            }
        }
        None
    }

    /// A click on a layer surface that takes the keyboard on demand gives
    /// it the keyboard.
    pub fn layer_click(&mut self, pos: Point<f64, Logical>) {
        let Some((_, _, layer)) =
            self.layer_surface_under(pos, &[ABOVE[0], ABOVE[1], BELOW[0], BELOW[1]])
        else {
            return;
        };
        if layer.cached_state().keyboard_interactivity == KeyboardInteractivity::OnDemand
            && let Some(keyboard) = self.seat.get_keyboard()
        {
            keyboard.set_focus(
                self,
                Some(layer.wl_surface().clone()),
                SERIAL_COUNTER.next_serial(),
            );
            self.protocols.layer_focus = true;
        }
    }

    /// A click elsewhere than on a layer surface: the windows get the
    /// keyboard back from an on-demand one.
    pub fn layer_release_keyboard(&mut self) {
        if self.protocols.layer_focus && self.exclusive_layer().is_none() {
            self.protocols.layer_focus = false;
        }
    }

    /// The surfaces of `layers` on `output`, front first.
    pub fn layer_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        layers: &[Layer],
    ) -> Vec<WaylandSurfaceRenderElement<R>>
    where
        R: Renderer + ImportAll,
        R::TextureId: Texture + Clone + 'static,
    {
        let scale = Scale::from(output.current_scale().fractional_scale());
        let map = layer_map_for_output(output);
        let mut out = Vec::new();
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let Some(geometry) = map.layer_geometry(surface) else {
                    continue;
                };
                let location = geometry.loc.to_physical_precise_round(scale);
                out.extend(AsRenderElements::<R>::render_elements::<
                    WaylandSurfaceRenderElement<R>,
                >(surface, renderer, location, scale, 1.0));
            }
        }
        out
    }
}
