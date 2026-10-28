//! What an output shows, front first: the cursor, the layer surfaces above
//! the windows, a fullscreen window, the snap preview, windows leaving,
//! the windows with their grips, rims and shadows (opening, settling and
//! wobbling as they go), and the layers under them; and what clients are
//! told once a frame is out.

use std::time::Duration;

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, RendererSuper, Texture,
        element::{
            AsRenderElements, Kind, RenderElementStates, default_primary_scanout_output_compare,
            render_elements,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            texture::TextureRenderElement,
            utils::{Relocate, RelocateRenderElement, RescaleRenderElement},
        },
    },
    desktop::{
        PopupManager, layer_map_for_output,
        utils::{
            OutputPresentationFeedback, send_frames_surface_tree, surface_primary_scanout_output,
            update_surface_primary_scanout_output,
        },
    },
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Monotonic, Physical, Point, Rectangle, Scale},
    wayland::{
        compositor::{SurfaceData, send_surface_state},
        fractional_scale::with_fractional_scale,
        presentation::Refresh,
        shell::wlr_layer::Layer,
    },
};

use crate::{
    State,
    cursor::CursorElement,
    decoration::{
        self, FramePiece, RimKind, Rounded, ShadowPiece, ShadowStyle, grip_element, handle_element,
        preview_elements, rim_elements, shadow_elements,
    },
    model::{Mode, WindowId},
    motion::lerp,
    wobble::{Bend, BentElement, Picture},
};

/// The background, until wallpapers: the dark theme's base colour.
pub const BACKGROUND: [f32; 4] = [0.055, 0.059, 0.071, 1.0];

/// A window's surface, stretched and moved to where it is drawn, its
/// corners rounded.
pub type WindowSurface<R> =
    Rounded<RelocateRenderElement<RescaleRenderElement<WaylandSurfaceRenderElement<R>>>>;

render_elements! {
    pub OutputElements<R> where R: ImportAll + ImportMem + Bend;
    Cursor = CursorElement<R>,
    /// Layer surfaces.
    Surface = WaylandSurfaceRenderElement<R>,
    Window = WindowSurface<R>,
    Frame = FramePiece<R>,
    Shadow = ShadowPiece<R>,
    /// The last picture of a window leaving.
    Picture = Rounded<TextureRenderElement<<R as RendererSuper>::TextureId>>,
    /// A window wobbling.
    Bent = BentElement,
}

/// What an output shows, element by element.
pub type OutputElement<R> = OutputElements<R>;

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

/// How a window looks at a moment.
pub(crate) struct Look {
    /// Where it is drawn, global.
    pub visual: Rectangle<f64, Logical>,
    pub alpha: f32,
    focus: f32,
    pulse: f32,
    grip: f32,
    decorated: bool,
    shadow: ShadowStyle,
    own_shadow: bool,
}

impl State {
    pub(crate) fn look(&self, id: WindowId, now: Duration) -> Option<Look> {
        let managed = self.windows.get(&id)?;
        let mut visual = self.visual_rect(id, now)?;
        let mut alpha = 1.0;
        if let Some(opening) = managed.opening {
            alpha = opening.linear(now) as f32;
            if !self.reduced_motion() {
                let p = opening.at(now);
                let k = lerp(crate::motion::OPEN_SCALE, 1.0, p);
                let size = visual.size.upscale(k);
                visual.loc += (visual.size.to_point() - size.to_point()).downscale(2.0);
                visual.size = size;
                alpha = p.clamp(0.0, 1.0) as f32;
            }
        }
        let focus = managed.focus_look(now);
        let decorated = self.model.mode(id) != Mode::Fullscreen;
        // A client drawing its own shadow (a floating GTK window) keeps it:
        // the desktop's would double it.
        let (bbox, geometry) = (managed.window.bbox(), managed.window.geometry());
        let own_shadow = bbox.loc.x < geometry.loc.x - 2 || bbox.size.w > geometry.size.w + 4;
        let shadow = if self.moving == Some(id) {
            ShadowStyle::MOVING
        } else {
            let (a, b) = (ShadowStyle::UNFOCUSED, ShadowStyle::FOCUSED);
            ShadowStyle {
                alpha: a.alpha + (b.alpha - a.alpha) * focus,
                ..a
            }
        };
        let grip = if decorated && !self.has_own_header(id) {
            managed.grip.alpha(now)
        } else {
            0.0
        };
        Some(Look {
            visual,
            alpha,
            focus,
            pulse: managed.pulse_look(now),
            grip,
            decorated,
            shadow,
            own_shadow,
        })
    }

    /// The window's surfaces drawn into `look.visual`, output-local from
    /// `origin` (global), front first: popups first.
    fn window_parts<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        look: &Look,
        origin: Point<f64, Logical>,
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        let window = &managed.window;
        let Some(loc) = self.space.element_location(window) else {
            return;
        };
        let geometry = window.geometry();
        let actual = Rectangle::new(loc, geometry.size).to_f64();
        let visual = look.visual;
        // From the window as it is to where it is drawn.
        let k = Scale::from((
            visual.size.w / actual.size.w.max(1.0),
            visual.size.h / actual.size.h.max(1.0),
        ));
        let at = |p: Point<f64, Logical>| -> Point<i32, Physical> {
            (p - origin).to_physical_precise_round(scale)
        };
        let actual_loc = at(actual.loc);
        let offset = at(visual.loc) - actual_loc;
        let surface_origin = at(loc.to_f64() - geometry.loc.to_f64());
        let clip: Rectangle<i32, Physical> = Rectangle::new(
            (visual.loc - origin).to_physical_precise_round(scale),
            visual.size.to_physical_precise_round(scale),
        );
        let radius = if look.decorated {
            (decoration::RADIUS * scale * k.x.min(k.y)).round() as i32
        } else {
            0
        };
        let wrap = |e: WaylandSurfaceRenderElement<R>, radius: i32| {
            let scaled = RescaleRenderElement::from_element(e, actual_loc, k);
            let moved = RelocateRenderElement::from_element(scaled, offset, Relocate::Relative);
            OutputElements::Window(Rounded::new(moved, clip, radius))
        };
        let sc = Scale::from(scale);
        if let Some(toplevel) = window.toplevel() {
            let surface = toplevel.wl_surface();
            for (popup, popup_offset) in PopupManager::popups_for_surface(surface) {
                let at_popup = (geometry.loc + popup_offset - popup.geometry().loc)
                    .to_f64()
                    .to_physical_precise_round(scale);
                let popups: Vec<WaylandSurfaceRenderElement<R>> = render_elements_from_surface_tree(
                    renderer,
                    popup.wl_surface(),
                    surface_origin + at_popup,
                    sc,
                    look.alpha,
                    Kind::Unspecified,
                );
                // Menus are not cut by the window's corners.
                out.extend(popups.into_iter().map(|e| wrap(e, 0)));
            }
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> = render_elements_from_surface_tree(
                renderer,
                surface,
                surface_origin,
                sc,
                look.alpha,
                Kind::Unspecified,
            );
            out.extend(surfaces.into_iter().map(|e| wrap(e, radius)));
        } else {
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(
                    window,
                    renderer,
                    surface_origin,
                    sc,
                    look.alpha,
                );
            out.extend(surfaces.into_iter().map(|e| wrap(e, radius)));
        }
    }

    /// A window with its frame, front first: its grip, its rims, its
    /// surfaces and its shadow.
    fn framed_window<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        look: &Look,
        origin: Point<f64, Logical>,
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        let ids = &managed.ids;
        let rect = Rectangle::new(look.visual.loc - origin, look.visual.size);
        let textures = &self.textures;
        if look.decorated {
            if let Some(grip) =
                grip_element(renderer, textures, ids, rect, scale, look.grip * look.alpha)
            {
                out.push(OutputElements::Frame(grip));
            }
            let focused_width = if self.config.appearance.high_contrast {
                2.0
            } else {
                1.0
            };
            for (kind, width, alpha) in [
                (RimKind::Pulse, 1.0, look.pulse),
                (RimKind::Accent, focused_width, look.focus),
                (RimKind::Plain, 1.0, 1.0 - look.focus),
            ] {
                out.extend(
                    rim_elements(
                        renderer,
                        textures,
                        ids,
                        kind,
                        width,
                        rect,
                        scale,
                        alpha * look.alpha,
                    )
                    .into_iter()
                    .map(OutputElements::Frame),
                );
            }
        }
        self.window_parts(renderer, id, look, origin, scale, out);
        if look.decorated && !look.own_shadow {
            out.extend(
                shadow_elements(
                    renderer,
                    textures,
                    ids,
                    look.shadow,
                    rect,
                    scale,
                    look.alpha,
                )
                .into_iter()
                .map(OutputElements::Shadow),
            );
        }
    }

    /// A window wobbling: drawn into its picture and bent. `None` when the
    /// renderer cannot: it is then drawn rigid.
    fn bent_window<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        look: &Look,
        output_origin: Point<f64, Logical>,
        scale: f64,
    ) -> Option<BentElement>
    where
        R: Renderer + ImportAll + ImportMem + Bend,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (wid, jelly) = self.wobble.as_ref()?;
        if *wid != id || look.alpha < 1.0 {
            return None;
        }
        let (w, h) = jelly.size();
        if (w - look.visual.size.w).abs() > 0.5 || (h - look.visual.size.h).abs() > 0.5 {
            return None;
        }
        let style = look.shadow;
        let margin = (1.25 * style.blur + style.dy + 2.0).ceil();
        let extent = Rectangle::new(
            (-margin, -margin).into(),
            (w + 2.0 * margin, h + 2.0 * margin).into(),
        );
        // The picture's corner is the origin its parts are drawn from.
        let picture_origin = look.visual.loc + extent.loc;
        let mut parts = Vec::new();
        self.framed_window(renderer, id, look, picture_origin, scale, &mut parts);
        let picture = Picture {
            window: Rectangle::new(look.visual.loc - output_origin, look.visual.size),
            extent,
            scale,
        };
        renderer.bend(jelly, &picture, &parts, 1.0)
    }

    /// What `output` shows, front first. `cursor`: whether the compositor
    /// draws the cursor (nested, the host does).
    pub fn output_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        cursor: bool,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        self.grabber.ready(renderer);
        let now = self.now();
        let mut elements: Vec<OutputElement<R>> = Vec::new();
        let Some(area) = self.space.output_geometry(output) else {
            return elements;
        };
        let origin = area.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let sc = Scale::from(scale);
        if cursor {
            elements.extend(
                self.cursor_elements(renderer, output)
                    .into_iter()
                    .map(OutputElements::Cursor),
            );
        }
        let layers = |renderer: &mut R, which: Layer, out: &mut Vec<OutputElement<R>>| {
            let map = layer_map_for_output(output);
            let on: Vec<_> = map.layers_on(which).rev().cloned().collect();
            for layer in on {
                let Some(geometry) = map.layer_geometry(&layer) else {
                    continue;
                };
                let loc = geometry.loc.to_physical_precise_round(scale);
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                    AsRenderElements::<R>::render_elements(&layer, renderer, loc, sc, 1.0);
                out.extend(surfaces.into_iter().map(OutputElements::Surface));
            }
        };
        layers(renderer, Layer::Overlay, &mut elements);
        // The windows on this output, front first; a fullscreen one on top
        // goes over the bars.
        let looks: Vec<(WindowId, Look)> = self
            .model
            .stack()
            .iter()
            .rev()
            .copied()
            .filter(|id| !self.model.is_minimized(*id))
            .filter_map(|id| Some((id, self.look(id, now)?)))
            .filter(|(_, look)| {
                let reach = 2.0 * ShadowStyle::MOVING.blur;
                let around = Rectangle::new(
                    look.visual.loc - Point::from((reach, reach)),
                    look.visual.size + (2.0 * reach, 2.0 * reach).into(),
                );
                around.overlaps(area.to_f64())
            })
            .collect();
        let fullscreen_on_top = looks
            .first()
            .is_some_and(|(id, _)| self.model.mode(*id) == Mode::Fullscreen);
        let mut windows = Vec::new();
        for (id, look) in &looks {
            match self.bent_window(renderer, *id, look, origin, scale) {
                Some(bent) => windows.push(OutputElements::Bent(bent)),
                None => self.framed_window(renderer, *id, look, origin, scale, &mut windows),
            }
        }
        if fullscreen_on_top {
            elements.append(&mut windows);
        }
        layers(renderer, Layer::Top, &mut elements);
        // Where a dragged window would snap, above the windows.
        let preview = self
            .preview
            .as_ref()
            .filter(|p| p.shown(now))
            .map(|p| (p.rect, p.alpha(now)))
            .or_else(|| {
                self.preview_out
                    .as_ref()
                    .map(|(rect, t)| (*rect, 1.0 - t.at(now) as f32))
            });
        if let Some((rect, alpha)) = preview {
            let r = crate::windows::rectangle(rect).to_f64();
            let r = Rectangle::new(r.loc - origin, r.size);
            elements.extend(
                preview_elements(renderer, &self.textures, &self.preview_ids, r, scale, alpha)
                    .into_iter()
                    .map(OutputElements::Frame),
            );
        }
        // The handle of a snapped pair's seam, where the pair is now.
        let handle = self.seam_handle.and_then(|(seam, handle)| {
            let live = self.live_seam(seam.first, seam.second)?;
            Some((live, handle.alpha(now)))
        });
        if let Some((seam, alpha)) = handle {
            let (x, y) = seam.centre();
            if area.to_f64().contains((x, y)) {
                let centre = Point::from((x, y)) - origin;
                elements.extend(
                    handle_element(
                        renderer,
                        &self.textures,
                        &self.seam_handle_id,
                        centre,
                        seam.vertical,
                        scale,
                        alpha,
                    )
                    .map(OutputElements::Frame),
                );
            }
        }
        // Windows leaving, from their last frame.
        for leaving in self.leaving.iter().rev() {
            let p = leaving.tween.at(now);
            let rect = Rectangle::<f64, Logical>::new(
                (
                    lerp(leaving.from.loc.x, leaving.to.loc.x, p),
                    lerp(leaving.from.loc.y, leaving.to.loc.y, p),
                )
                    .into(),
                (
                    lerp(leaving.from.size.w, leaving.to.size.w, p),
                    lerp(leaving.from.size.h, leaving.to.size.h, p),
                )
                    .into(),
            );
            if !rect.overlaps(area.to_f64()) {
                continue;
            }
            let alpha = (1.0 - leaving.fade.linear(now)) as f32;
            let local = Rectangle::new(rect.loc - origin, rect.size);
            let clip: Rectangle<i32, Physical> = Rectangle::new(
                local.loc.to_physical_precise_round(scale),
                local.size.to_physical_precise_round(scale),
            );
            let k = (local.size.w / leaving.picture.size.w.max(1.0))
                .min(local.size.h / leaving.picture.size.h.max(1.0));
            let radius = (decoration::RADIUS * scale * k).round() as i32;
            elements.extend(
                leaving
                    .picture
                    .stretched(renderer, local, scale, alpha)
                    .into_iter()
                    .map(|e| OutputElements::Picture(Rounded::new(e, clip, radius))),
            );
        }
        elements.append(&mut windows);
        layers(renderer, Layer::Bottom, &mut elements);
        layers(renderer, Layer::Background, &mut elements);
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
