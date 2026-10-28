//! wlr-layer-shell: the shell's bar and overlays, and third-party bars,
//! launchers and wallpapers. Their surfaces sit in four layers: background
//! and bottom under the windows, top and overlay above them. Exclusive
//! zones come off the work area. A surface may take the keyboard, always
//! (exclusive) or when clicked (on demand).

use smithay::{
    delegate_layer_shell,
    desktop::{LayerSurface, PopupKind, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::{IsAlive, Logical, Point, SERIAL_COUNTER},
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

use crate::{State, focus::KeyboardFocus};

/// The layers above the windows, front first.
const ABOVE: [Layer; 2] = [Layer::Overlay, Layer::Top];
const BELOW: [Layer; 2] = [Layer::Bottom, Layer::Background];

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
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
                self.layer_keyboard();
                self.redraw = true;
            }
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(popup)) {
            tracing::warn!(%err, "could not track a layer surface's popup");
        }
    }
}
delegate_layer_shell!(State);

/// A commit of a layer surface: arranged again, configured the first time.
pub fn handle_commit(state: &mut State, surface: &WlSurface) {
    let outputs: Vec<Output> = state.space.outputs().cloned().collect();
    for output in outputs {
        let map = layer_map_for_output(&output);
        let Some(layer) = map
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .cloned()
        else {
            continue;
        };
        drop(map);
        let before = state.work_area(&output);
        layer_map_for_output(&output).arrange();
        if state.work_area(&output) != before {
            // Snapped, maximised and fullscreen windows follow.
            state.rearrange();
        }
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
        state.layer_keyboard();
        return;
    }
}

impl State {
    /// The layer surface under `pos` in `layers` (front first), and where
    /// the surface found is.
    pub fn layer_surface_under(
        &self,
        pos: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(WlSurface, Point<f64, Logical>, LayerSurface)> {
        let output = self.space.output_under(pos).next()?;
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

    /// A surface of the top or overlay layer asking for the keyboard always
    /// gets it; when none does any more, the focused window gets it back.
    pub fn layer_keyboard(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        match self.exclusive_layer() {
            Some(layer) => {
                let focus = KeyboardFocus::Wayland(layer.wl_surface().clone());
                if keyboard.current_focus().as_ref() != Some(&focus) {
                    keyboard.set_focus(self, Some(focus), serial);
                }
            }
            None => {
                // An on-demand surface clicked keeps it; one that went, or
                // that no longer asks, gives it back to the windows.
                let Some(focus) = keyboard.current_focus() else {
                    return;
                };
                let kept = match &focus {
                    KeyboardFocus::Wayland(s) if s.alive() => self.layer_of(s).is_none_or(|l| {
                        l.cached_state().keyboard_interactivity == KeyboardInteractivity::OnDemand
                    }),
                    KeyboardFocus::Wayland(_) => false,
                    KeyboardFocus::X11(x11) => x11.alive(),
                };
                if !kept {
                    let focused = self.model.focused();
                    self.focus(focused);
                }
            }
        }
    }

    /// Whether a layer surface above the windows holds the keyboard for
    /// good: windows may not take it.
    pub fn keyboard_held_by_layer(&self) -> bool {
        self.exclusive_layer().is_some()
    }

    /// The layer surface whose main surface is `surface`.
    fn layer_of(&self, surface: &WlSurface) -> Option<LayerSurface> {
        self.space.outputs().find_map(|o| {
            layer_map_for_output(o)
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .cloned()
        })
    }

    /// A press over a layer surface: one that takes the keyboard on demand
    /// gets it. Whether there was one under `pos`.
    pub fn layer_click(&mut self, pos: Point<f64, Logical>) -> bool {
        let found = self.layer_surface_under(pos, &ABOVE).or_else(|| {
            self.space
                .element_under(pos)
                .is_none()
                .then(|| self.layer_surface_under(pos, &BELOW))
                .flatten()
        });
        let Some((_, _, layer)) = found else {
            return false;
        };
        if layer.cached_state().keyboard_interactivity == KeyboardInteractivity::OnDemand
            && let Some(keyboard) = self.seat.get_keyboard()
        {
            keyboard.set_focus(
                self,
                Some(KeyboardFocus::Wayland(layer.wl_surface().clone())),
                SERIAL_COUNTER.next_serial(),
            );
        }
        true
    }
}
