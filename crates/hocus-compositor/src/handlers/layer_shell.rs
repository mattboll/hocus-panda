//! wlr-layer-shell: the shell's bar and overlays, and third-party bars,
//! launchers and wallpapers. Their surfaces sit in four layers: background
//! and bottom under the windows, top and overlay above them. Exclusive
//! zones come off the work area. A surface may take the keyboard, always
//! (exclusive) or when clicked (on demand).

use std::time::Duration;

use smithay::{
    backend::{input::KeyState, renderer::utils::with_renderer_surface_state},
    delegate_layer_shell,
    desktop::{LayerSurface, PopupKind, WindowSurfaceType, layer_map_for_output},
    input::keyboard::Keycode,
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::{IsAlive, Logical, Point, SERIAL_COUNTER, Size},
    wayland::{
        compositor::with_states,
        shell::{
            wlr_layer::{
                Anchor, ExclusiveZone, KeyboardInteractivity, Layer,
                LayerSurface as WlrLayerSurface, LayerSurfaceCachedState, LayerSurfaceData,
                Margins, WlrLayerShellHandler, WlrLayerShellState,
            },
            xdg::PopupSurface,
        },
    },
};

use crate::{State, focus::KeyboardFocus, front::Pick, state::Coming};

/// The layers above the windows, front first, for the keyboard.
const ABOVE: [Layer; 2] = [Layer::Overlay, Layer::Top];

/// Where layer surfaces stand to the windows for the pointer: the overlay
/// and top layers above them, but the bar's own surfaces under them unless
/// the bar is brought in front (#519, `crate::front`); the bottom and
/// background layers under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stand {
    Above,
    Below,
}

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.protocols.layer_shell_state
    }

    /// On the output asked for; asked for on none, one of the bar's own
    /// surfaces on a screen whose bar lacks it, any other surface on the
    /// screen under the pointer.
    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = match output.as_ref() {
            // A screen unplugged as the client asked: its global is still
            // there a moment, disabled, but the surface would show nowhere.
            Some(asked) => Output::from_resource(asked).filter(|o| self.is_plugged(o)),
            None => {
                tracing::debug!(namespace, "a layer surface asked for on no screen");
                self.screen_for(&namespace)
            }
        };
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
                self.layer_gone(&layer);
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

/// The bar's own surfaces, one of each per bar.
const BAR_PARTS: [&str; 8] = [
    hocus_link::NAMESPACE,
    hocus_link::CLOCK_NAMESPACE,
    hocus_link::LEVELS_NAMESPACE,
    hocus_link::CONNECTIONS_NAMESPACE,
    hocus_link::DESKTOPS_NAMESPACE,
    hocus_link::AGENTS_NAMESPACE,
    hocus_link::TASKS_NAMESPACE,
    hocus_link::FOOTER_NAMESPACE,
];

impl State {
    /// The screen for a layer surface of `namespace` asked for on none.
    /// One of the bar's own surfaces goes to a screen given a bar that
    /// does not have that surface yet, the one under the pointer if it is
    /// one of them: a screen just plugged in may get its bar's surfaces
    /// before the shell knows its name (#540). On a screen that has its
    /// own already it would be a second bar: it goes nowhere. Any other
    /// surface goes under the pointer.
    fn screen_for(&mut self, namespace: &str) -> Option<Output> {
        if !BAR_PARTS.contains(&namespace) {
            return self.output_under_pointer();
        }
        self.panel.asked_nowhere += 1;
        let lacking: Vec<Output> = self
            .space
            .outputs()
            .filter(|o| {
                self.panel.bars.contains_key(&o.name())
                    && !layer_map_for_output(o)
                        .layers()
                        .any(|l| l.namespace() == namespace)
            })
            .cloned()
            .collect();
        self.output_under_pointer()
            .filter(|o| lacking.contains(o))
            .or_else(|| lacking.into_iter().next())
    }

    /// `layer`, unmapped, no longer shows: what showed it or depended on
    /// it lets it go.
    pub(crate) fn layer_gone(&mut self, layer: &LayerSurface) {
        self.popover_gone(layer);
        self.launcher_gone(layer);
        self.actions_gone(layer);
        self.capture_toolbar_gone(layer);
        self.share_chooser_gone(layer);
        self.auth_gone(layer);
        self.capture_after_gone(layer);
        self.peek_gone(layer);
        self.bubble_gone(layer);
        self.layer_keyboard();
        self.redraw = true;
    }
}

/// Margins that would leave layer surface `surface` no room on `output`
/// held to it, before it is arranged: smithay would configure it with a
/// negative size, which the client reads as a huge one and dies of (#942:
/// a bar's piece, its margins asked again at a new text size a moment
/// before its column gives it its new place).
fn hold_margins(surface: &WlSurface, output: &Output, whole: Option<Size<i32, Logical>>) {
    let zone = layer_map_for_output(output).non_exclusive_zone().size;
    with_states(surface, |states| {
        let mut cached = states.cached_state.get::<LayerSurfaceCachedState>();
        let state = cached.current();
        let room = match state.exclusive_zone {
            ExclusiveZone::DontCare => whole.unwrap_or(zone),
            _ => zone,
        };
        let m = state.margin;
        let asked = (m.top, m.right, m.bottom, m.left);
        let (top, right, bottom, left) = held(asked, state.anchor, (room.w, room.h));
        if (top, right, bottom, left) != asked {
            tracing::debug!(
                ?asked,
                top,
                right,
                bottom,
                left,
                "a layer surface's margins held to its screen"
            );
            state.margin = Margins {
                top,
                right,
                bottom,
                left,
            };
        }
    });
}

/// Margins (top, right, bottom, left) for a surface anchored to `anchor`,
/// each axis leaving at least a pixel of `room` (width, height); the
/// second edge's gives way first.
fn held(
    (mut top, mut right, mut bottom, mut left): (i32, i32, i32, i32),
    anchor: Anchor,
    (width, height): (i32, i32),
) -> (i32, i32, i32, i32) {
    let hold = |first: &mut i32, second: &mut i32, at_first: bool, at_second: bool, room: i32| {
        let mut over = i32::from(at_first) * *first + i32::from(at_second) * *second - (room - 1);
        for (side, anchored) in [(second, at_second), (first, at_first)] {
            if over > 0 && anchored && *side > 0 {
                let taken = over.min(*side);
                *side -= taken;
                over -= taken;
            }
        }
    };
    hold(
        &mut top,
        &mut bottom,
        anchor.contains(Anchor::TOP),
        anchor.contains(Anchor::BOTTOM),
        height,
    );
    hold(
        &mut left,
        &mut right,
        anchor.contains(Anchor::LEFT),
        anchor.contains(Anchor::RIGHT),
        width,
    );
    (top, right, bottom, left)
}

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
        match layer.namespace() {
            hocus_link::NAMESPACE => {
                state.panel.commits += 1;
                state.bar_committed(&layer);
            }
            hocus_link::CLOCK_NAMESPACE => state.panel.clock_commits += 1,
            hocus_link::LEVELS_NAMESPACE => state.panel.levels_commits += 1,
            hocus_link::IN_USE_NAMESPACE => state.panel.in_use_commits += 1,
            hocus_link::MEETING_NAMESPACE => state.panel.meeting_commits += 1,
            hocus_link::TODAY_NAMESPACE => state.panel.today_commits += 1,
            hocus_link::WINDOWS_NAMESPACE => state.panel.windows_commits += 1,
            hocus_link::CONNECTIONS_NAMESPACE => state.panel.connections_commits += 1,
            hocus_link::DESKTOPS_NAMESPACE => state.panel.desktops_commits += 1,
            hocus_link::AGENTS_NAMESPACE => state.agents.commits += 1,
            hocus_link::TASKS_NAMESPACE => state.panel.tasks_commits += 1,
            hocus_link::POPOVER_NAMESPACE => {
                state.panel.popover_commits += 1;
                state.popover_committed(&layer);
            }
            hocus_link::EMOJI_NAMESPACE => state.popover_committed(&layer),
            ns if hocus_link::is_project_surface(ns) => state.popover_committed(&layer),
            hocus_link::ACTIONS_NAMESPACE => {
                state.panel.actions_commits += 1;
                state.popover_committed(&layer);
            }
            hocus_link::PEEK_NAMESPACE => {
                state.panel.peek_commits += 1;
                state.peek_committed(&layer);
            }
            hocus_link::LAUNCHER_NAMESPACE => {
                state.panel.launcher_commits += 1;
                state.launcher_committed(&layer);
            }
            hocus_link::LAUNCHER_TOAST_NAMESPACE => state.launcher_toast_committed(&layer),
            hocus_link::CAPTURE_TOOLBAR_NAMESPACE => state.capture_toolbar_committed(&layer),
            hocus_link::SHARE_CHOOSER_NAMESPACE => state.share_chooser_committed(&layer),
            hocus_link::AUTH_NAMESPACE => state.auth_committed(&layer, &output),
            hocus_link::CAPTURE_THUMB_NAMESPACE | hocus_link::CAPTURE_EDITOR_NAMESPACE => {
                state.capture_after_committed(&layer);
            }
            ns if hocus_link::is_bubble(ns) => state.bubble_committed(&layer),
            ns if hocus_link::is_reminder(ns) => {
                state.panel.reminder_commits += 1;
                state.bubble_committed(&layer);
            }
            hocus_link::LOCK_NAMESPACE => state.lock_layer_committed(&layer, &output),
            hocus_link::CURTAIN_NAMESPACE => state.curtain_committed(&layer, &output),
            _ => {}
        }
        let whole = state.space.output_geometry(&output).map(|g| g.size);
        hold_margins(surface, &output, whole);
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
    /// The layer surface under `pos` that stands `stand` the windows
    /// (front first), and where the surface found is.
    pub fn layer_surface_under(
        &self,
        pos: Point<f64, Logical>,
        stand: Stand,
    ) -> Option<(WlSurface, Point<f64, Logical>, LayerSurface)> {
        let output = self.space.output_under(pos).next()?;
        let origin = self.space.output_geometry(output)?.loc;
        let front = self.bar_in_front(output);
        let passes = match stand {
            Stand::Above => [
                (Layer::Overlay, Pick::All),
                (Layer::Top, Pick::above(front)),
                (Layer::Bottom, Pick::Nothing),
            ],
            Stand::Below => [
                (Layer::Top, Pick::below(front)),
                (Layer::Bottom, Pick::All),
                (Layer::Background, Pick::All),
            ],
        };
        let map = layer_map_for_output(output);
        // Each surface's layer read once, not once a pass (#948): asked at
        // each turn of the loop, under the still pointer.
        let layers: Vec<(&LayerSurface, Layer)> = map.layers().map(|l| (l, l.layer())).collect();
        for (layer, pick) in passes {
            if pick == Pick::Nothing {
                continue;
            }
            let on = layers.iter().rev().filter(|(_, l)| *l == layer);
            for &(surface, _) in on {
                // The lock's surfaces take the pointer through the lock
                // alone, while it holds (7.1).
                if !pick.takes(surface.namespace())
                    || surface.namespace() == hocus_link::LOCK_NAMESPACE
                    || surface.namespace() == hocus_link::CURTAIN_NAMESPACE
                {
                    continue;
                }
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
        // The authentication dialog first, whatever else asks (7.5).
        if let Some(layer) = self.auth_layer() {
            return Some(layer);
        }
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
        // Locked (7.1): the lock decides, nobody else.
        if self.lock_keyboard() {
            return;
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        match self.exclusive_layer() {
            Some(layer) => {
                let surface = layer.wl_surface().clone();
                if self.keys_wait_for(&surface) {
                    // Nobody reads keys meanwhile: they wait for it.
                    if keyboard.current_focus().is_some() {
                        keyboard.set_focus(self, None, serial);
                    }
                    return;
                }
                let focus = KeyboardFocus::Wayland(surface.clone());
                if keyboard.current_focus().as_ref() != Some(&focus) {
                    keyboard.set_focus(self, Some(focus), serial);
                    // A bar holding the keyboard stands in front.
                    self.redraw = true;
                }
                // The keys typed as it came, now that it reads them.
                if let Coming::Waiting { keys, .. } =
                    std::mem::replace(&mut self.keyboard.coming, Coming::Late(surface))
                {
                    for (code, state, time) in keys {
                        self.key(code, state, time);
                    }
                }
            }
            None => {
                // A surface that went before it drew: the keys typed for
                // it go with it, and the windows get the keyboard back.
                let coming = std::mem::take(&mut self.keyboard.coming);
                if let Coming::Waiting { keys, .. } = coming {
                    tracing::debug!(keys = keys.len(), "a layer surface went before it drew");
                    if keyboard.current_focus().is_none() {
                        let focused = self.model.focused();
                        self.focus(focused);
                        self.redraw = true;
                    }
                    return;
                }
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
                    self.redraw = true;
                }
            }
        }
    }

    /// Whether the keys wait for `surface`, which asks for the keyboard
    /// always: until it has drawn, for [`Coming::PATIENCE`] at most (#821).
    fn keys_wait_for(&mut self, surface: &WlSurface) -> bool {
        let now = Duration::from(self.clock.now());
        let drawn = with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false);
        match &mut self.keyboard.coming {
            Coming::Late(late) if late == surface => false,
            Coming::Waiting {
                surface: s,
                since,
                keys,
            } if s == surface => {
                if drawn || now.saturating_sub(*since) < Coming::PATIENCE {
                    return !drawn;
                }
                tracing::warn!(
                    keys = keys.len(),
                    "a layer surface asking for the keyboard has not drawn in {:?}: \
                     its keys go to it as they come",
                    Coming::PATIENCE
                );
                false
            }
            coming => {
                if drawn {
                    *coming = Coming::Late(surface.clone());
                    return false;
                }
                *coming = Coming::Waiting {
                    surface: surface.clone(),
                    since: now,
                    keys: Vec::new(),
                };
                true
            }
        }
    }

    /// A key that waits for a layer surface still to draw (#821): kept,
    /// in its order, rather than given to nobody. Once waited for too
    /// long, the surface gets the keyboard and the keys kept, and this one
    /// goes on.
    pub(crate) fn key_waits(&mut self, code: Keycode, state: KeyState, time: u32) -> bool {
        if !matches!(self.keyboard.coming, Coming::Waiting { .. }) || self.locked() {
            return false;
        }
        let now = Duration::from(self.clock.now());
        if let Coming::Waiting { since, keys, .. } = &mut self.keyboard.coming
            && now.saturating_sub(*since) < Coming::PATIENCE
        {
            keys.push((code, state, time));
            return true;
        }
        self.layer_keyboard();
        false
    }

    /// Whether a layer surface above the windows holds the keyboard for
    /// good: windows may not take it.
    pub fn keyboard_held_by_layer(&self) -> bool {
        // Locked (7.1): no window takes it either.
        self.locked() || self.exclusive_layer().is_some()
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
        let found = self.layer_surface_under(pos, Stand::Above).or_else(|| {
            self.space
                .element_under(pos)
                .is_none()
                .then(|| self.layer_surface_under(pos, Stand::Below))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn margins(top: i32, right: i32, bottom: i32, left: i32) -> (i32, i32, i32, i32) {
        (top, right, bottom, left)
    }

    /// #942: margins past the screen are held so that a pixel is left; the
    /// others, and those of an edge not anchored, stay as asked.
    #[test]
    fn margins_past_the_screen_are_held_to_it() {
        let right_top = Anchor::TOP | Anchor::RIGHT;
        let room = (1920, 1080);
        assert_eq!(
            held(margins(1377, 12, 0, 0), right_top, room),
            margins(1079, 12, 0, 0)
        );
        assert_eq!(
            held(margins(700, 12, 0, 0), right_top, room),
            margins(700, 12, 0, 0)
        );
        // Both edges anchored: the bottom gives way first.
        let column = Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT;
        assert_eq!(
            held(margins(900, 12, 400, 0), column, room),
            margins(900, 12, 179, 0)
        );
        // An edge not anchored takes no room.
        assert_eq!(
            held(
                margins(0, 12, 5000, 3000),
                Anchor::TOP | Anchor::RIGHT,
                room
            ),
            margins(0, 12, 5000, 3000)
        );
    }
}
