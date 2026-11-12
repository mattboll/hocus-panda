//! The layer surfaces an output shows, layer by layer.

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{AsRenderElements, surface::WaylandSurfaceRenderElement},
    },
    desktop::layer_map_for_output,
    utils::{Rectangle, Scale},
    wayland::shell::wlr_layer::Layer,
};

use super::{Frame, OutputElement, OutputElements};
use crate::{
    State,
    decoration::{Outside, Rounded, shadow_elements},
    front::{Pick, in_the_bar},
    going::Shaders,
    wobble::Bend,
};

impl State {
    /// The surfaces of layer `which` that `pick` takes, added to `out`;
    /// where the bar's first surface is drawn among `out`, if it is.
    pub(super) fn layer_elements<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        which: Layer,
        pick: Pick,
        out: &mut Vec<OutputElement<R>>,
    ) -> Option<usize>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Frame {
            output,
            purpose,
            now,
            scale,
            ..
        } = *frame;
        let sc = Scale::from(scale);
        let mut bar_at = None;
        let map = layer_map_for_output(output);
        let on: Vec<_> = map.layers_on(which).rev().cloned().collect();
        let bar = map
            .layers()
            .find(|l| l.namespace() == hocus_link::NAMESPACE)
            .and_then(|l| map.layer_geometry(l))
            .map(|g| {
                Rectangle::new(
                    g.loc.to_physical_precise_round(scale),
                    g.size.to_physical_precise_round(scale),
                )
            });
        for layer in on.into_iter().filter(|l| pick.takes(l.namespace())) {
            // The bar's item of what is in use and of the recording
            // (5.10, 5.11) is never in a recording, a capture or a share.
            if !purpose.screen() && layer.namespace() == hocus_link::IN_USE_NAMESPACE {
                continue;
            }
            // The peek goes under the bar's surfaces (`peek_elements`),
            // the launcher and its toast over everything
            // (`launcher_elements`, `toast_elements`).
            if matches!(
                layer.namespace(),
                hocus_link::LOCK_NAMESPACE
                    | hocus_link::CURTAIN_NAMESPACE
                    | hocus_link::PEEK_NAMESPACE
                    | hocus_link::LAUNCHER_NAMESPACE
                    | hocus_link::LAUNCHER_TOAST_NAMESPACE
                    | hocus_link::ACTIONS_NAMESPACE
                    | hocus_link::CAPTURE_TOOLBAR_NAMESPACE
                    | hocus_link::SHARE_CHOOSER_NAMESPACE
                    | hocus_link::AUTH_NAMESPACE
                    | hocus_link::CAPTURE_THUMB_NAMESPACE
                    | hocus_link::CAPTURE_EDITOR_NAMESPACE
            ) {
                continue;
            }
            let Some(mut geometry) = map.layer_geometry(&layer) else {
                continue;
            };
            // The windows' and the music player's surfaces are kept
            // while they show nothing: transparent on a GPU, black
            // from a shell on the CPU, whose buffers have no alpha.
            if self.shell.software && hocus_link::shows_nothing(layer.namespace(), geometry.size.h)
            {
                continue;
            }
            // A popover moves in and out, and a notification's bubble;
            // they show from their first frame.
            // The emoji picker too (5.7).
            let popover = matches!(
                layer.namespace(),
                hocus_link::POPOVER_NAMESPACE | hocus_link::EMOJI_NAMESPACE
            );
            // Popovers and reminders' tabs slide from the bar, on its
            // side; the bubbles from the screen's right edge.
            let look = if popover {
                self.panel
                    .popovers
                    .look(&layer, now, self.popover_motion())
                    .map(|l| (l.alpha, l.shift * self.toward_bar(), l.rise))
            } else if hocus_link::slides_in(layer.namespace()) {
                let toward = if hocus_link::is_reminder(layer.namespace()) {
                    self.toward_bar()
                } else {
                    1.0
                };
                self.bubbles
                    .look(&layer, now, self.bubbles_slide())
                    .map(|l| (l.alpha, l.shift * toward, 0.0))
            } else {
                Some((1.0, 0.0, 0.0))
            };
            let alpha = match look {
                Some((alpha, shift, rise)) if alpha > 0.0 => {
                    geometry.loc.x += shift.round() as i32;
                    geometry.loc.y -= rise.round() as i32;
                    alpha
                }
                _ => continue,
            };
            let loc = geometry.loc.to_physical_precise_round(scale);
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(&layer, renderer, loc, sc, alpha);
            if bar_at.is_none() && in_the_bar(layer.namespace()) {
                bar_at = Some(out.len());
            }
            // The CPU renderer's buffers have no alpha: what the bar
            // leaves out at its corners would show black. The clock's,
            // the levels', the connections' and the desktops' surfaces
            // are opaque too, and cut to the bar: they change size in
            // their own commits when the form switches.
            // A bubble and the chip under them are cards of their own.
            let own = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
            let cut = match layer.namespace() {
                hocus_link::NAMESPACE | hocus_link::POPOVER_NAMESPACE => {
                    Some((own, hocus_link::RADIUS))
                }
                hocus_link::EMOJI_NAMESPACE => Some((own, hocus_link::EMOJI_RADIUS)),
                ns if hocus_link::is_bubble(ns) => Some((own, hocus_link::RADIUS)),
                ns if hocus_link::is_reminder(ns) => Some((own, hocus_link::REMINDER_RADIUS)),
                hocus_link::BUBBLES_WAITING_NAMESPACE => Some((own, 10.0)),
                hocus_link::TIP_NAMESPACE => Some((own, hocus_link::TIP_RADIUS)),
                hocus_link::MCP_TAG_NAMESPACE => Some((own, hocus_link::MCP_TAG_RADIUS)),
                hocus_link::MCP_TOAST_NAMESPACE => Some((own, hocus_link::MCP_TOAST_RADIUS)),
                hocus_link::CLOCK_NAMESPACE
                | hocus_link::LEVELS_NAMESPACE
                | hocus_link::CONNECTIONS_NAMESPACE
                | hocus_link::DESKTOPS_NAMESPACE
                | hocus_link::AGENTS_NAMESPACE
                | hocus_link::TASKS_NAMESPACE
                | hocus_link::IN_USE_NAMESPACE
                | hocus_link::MEETING_NAMESPACE
                | hocus_link::TODAY_NAMESPACE
                | hocus_link::WINDOWS_NAMESPACE
                | hocus_link::MUSIC_NAMESPACE
                | hocus_link::BELL_NAMESPACE
                | hocus_link::TRAY_NAMESPACE => bar.map(|b| (b, hocus_link::RADIUS)),
                _ => None,
            };
            if let Some((clip, radius)) = cut.filter(|_| self.shell.software) {
                let radius = (radius * scale).round() as i32;
                out.extend(surfaces.into_iter().map(|e| {
                    OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
                }));
            } else {
                out.extend(surfaces.into_iter().map(OutputElements::Surface));
            }
            // The popover's shadow, right under it: over the bar, as
            // the mockup's popover's is.
            if let Some(shown) = self.panel.popovers.shown(&layer).filter(|_| popover) {
                out.extend(
                    shadow_elements(
                        renderer,
                        &self.textures,
                        &shown.ids,
                        shown.shadow(),
                        geometry.to_f64(),
                        scale,
                        alpha,
                    )
                    .into_iter()
                    .map(OutputElements::Shadow),
                );
            }
        }
        bar_at
    }
}
