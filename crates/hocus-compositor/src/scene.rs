//! What an output shows, front to back: the cursor, then each window from
//! the top with its focus ring, its content with rounded corners, and its
//! shadow. The headless and nested backends draw it, and screenshots too.

use hocus_core::Zone;
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement,
            utils::{RelocateRenderElement, RescaleRenderElement},
        },
    },
    desktop::Window,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use crate::{
    State,
    cursor::CursorElement,
    decoration::{Rounded, ring_elements, shadow_elements},
    desktops::{model_id, rect},
    handlers::layer_shell::LayerKind as Layer,
    window_mode::WindowMode,
};

render_elements! {
    pub SceneElement<R> where R: ImportAll + ImportMem;
    Cursor=CursorElement<R>,
    Window=Rounded<WaylandSurfaceRenderElement<R>>,
    /// A window drawn larger or smaller than it is: opening (#33).
    Scaled=RescaleRenderElement<Rounded<WaylandSurfaceRenderElement<R>>>,
    Frame=MemoryRenderBufferRenderElement<R>,
    Solid=SolidColorRenderElement,
}

render_elements! {
    /// What an output draws: the scene, the scene magnified (#78), or
    /// another screen's scaled to fit (#125).
    pub OutputElement<R> where R: ImportAll + ImportMem;
    Plain=SceneElement<R>,
    Magnified=RescaleRenderElement<SceneElement<R>>,
    Mirrored=RelocateRenderElement<RescaleRenderElement<SceneElement<R>>>,
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

    /// Everything `output` shows, front first; magnified around the
    /// pointer when the magnifier is on.
    pub fn scene_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        cursor: bool,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if let Some(source) = self.mirror_source(output) {
            return self.mirror_elements(renderer, output, &source, cursor);
        }
        let parts = self.scene_parts(renderer, output, cursor);
        let zoom = self.comfort.zoom;
        let area = self.space.output_geometry(output);
        let pointer = self.seat.get_pointer().map(|p| p.current_location());
        let origin = match (area, pointer) {
            (Some(area), Some(at)) if zoom > 1.0 && area.to_f64().contains(at) => {
                let scale = output.current_scale().fractional_scale();
                Some((at - area.loc.to_f64()).to_physical(scale).to_i32_round())
            }
            _ => None,
        };
        match origin {
            None => parts.into_iter().map(OutputElement::Plain).collect(),
            Some(origin) => parts
                .into_iter()
                .map(|e| {
                    OutputElement::Magnified(RescaleRenderElement::from_element(e, origin, zoom))
                })
                .collect(),
        }
    }

    /// Everything `output` shows, front first.
    pub(crate) fn scene_parts<R>(
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
        // Locked: the lock alone, whatever else there is.
        if self.locked() {
            let (client, ours) = self.lock_elements(renderer, output);
            elements.extend(client.into_iter().map(|e| {
                SceneElement::Window(Rounded::new(
                    e,
                    Rectangle::from_size((i32::MAX / 4, i32::MAX / 4).into()),
                    0,
                ))
            }));
            elements.extend(ours.into_iter().map(SceneElement::Frame));
            return elements;
        }
        // What to share, asked by a portal.
        elements.extend(
            self.share_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        // The system's password dialog, over everything but the cursor.
        elements.extend(
            self.auth_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        // Leaving: the countdown, over everything else.
        elements.extend(
            self.leave_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        // Choosing what a screenshot takes, and its flash.
        elements.extend(self.screenshot_elements(renderer, output));
        // What is recorded, and the recording's bar.
        elements.extend(self.recording_elements(renderer, output));
        // Each screen's name, while identifying.
        elements.extend(
            self.identify_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        // The card asking what to put back.
        elements.extend(
            self.card_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        // The curtain over everything but the cursor.
        elements.extend(
            self.curtain_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        if let Some((preview_output, zone)) = &self.snap_preview
            && preview_output == output
        {
            let target = zone.rect(rect(self.work_area(output)), self.config.windows.gaps.inner);
            let local = Rectangle::<i32, Logical>::new(
                Point::from((target.x, target.y)) - area.loc,
                (target.width, target.height).into(),
            );
            elements.extend(
                ring_elements(
                    renderer,
                    &self.textures,
                    &self.style,
                    local.to_f64(),
                    scale,
                    1.0,
                )
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
        // Layer surfaces as a frame: no corners.
        let flat = |e| {
            SceneElement::Window(Rounded::new(
                e,
                Rectangle::from_size((i32::MAX / 4, i32::MAX / 4).into()),
                0,
            ))
        };
        let fullscreen = self.fullscreen_on(output);
        // Overlay surfaces (layer-shell) over everything but the cursor.
        elements.extend(
            self.layer_elements(renderer, output, &[Layer::Overlay])
                .into_iter()
                .map(flat),
        );
        // The launcher over everything; quick settings over the screen
        // they dim; toasts, then the column, above windows; the column
        // under a fullscreen one.
        elements.extend(
            self.launcher_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        let (sheet, dim) = self.quick_elements(renderer, output);
        elements.extend(sheet.into_iter().map(SceneElement::Frame));
        elements.extend(dim.map(SceneElement::Solid));
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
        // Third-party bars above the windows, but under a fullscreen one.
        if !fullscreen {
            elements.extend(
                self.layer_elements(renderer, output, &[Layer::Top])
                    .into_iter()
                    .map(flat),
            );
        }
        let focused = self.focused_window();
        let windows: Vec<&Window> = self.space.elements().collect();
        // Focus mode: a veil over every window but the focused one, and
        // over what is behind them.
        let veil = |elements: &mut Vec<SceneElement<R>>| {
            let mut buffer = self.comfort.veil.clone();
            buffer.resize(area.size);
            elements.push(SceneElement::Solid(SolidColorRenderElement::from_buffer(
                &buffer,
                Point::<i32, Physical>::default(),
                scale,
                1.0,
                Kind::Unspecified,
            )));
        };
        let mut veiled = !self.comfort.focus_mode;
        let focused_here = focused
            .as_ref()
            .is_some_and(|f| windows.contains(&f) && self.output_of(f) == Some(output));
        if !veiled && !focused_here {
            veil(&mut elements);
            veiled = true;
        }
        for window in windows.into_iter().rev() {
            let Some(mut location) = self.space.element_location(window) else {
                continue;
            };
            // The drop-down terminal slides from above its place.
            if let Some(dy) = model_id(window).and_then(|id| self.dropdown_offset(id)) {
                location.y += dy;
            }
            let geometry = window.geometry();
            let rect = Rectangle::<i32, Logical>::new(location - area.loc, geometry.size);
            let bbox = Rectangle::new(
                location - geometry.loc + window.bbox().loc,
                window.bbox().size,
            );
            if !bbox.overlaps(area) && !rect.overlaps(Rectangle::from_size(area.size)) {
                continue;
            }
            let id = model_id(window);
            let bar = id.map_or(0, |id| self.bar_height(id));
            // The frame: the bar above the client.
            let frame = Rectangle::<i32, Logical>::new(
                rect.loc - Point::from((0, bar)),
                (rect.size.w, rect.size.h + bar).into(),
            );
            // Its motion (#33): the window, its ring and its shadow fade
            // and scale about its middle.
            let managed = id.and_then(|id| self.windows.get(&id));
            let (alpha, grow) = managed.map_or((1.0, 1.0), |m| m.motion.at(self.clock.now()));
            let grow = f64::from(grow);
            let middle = frame.to_f64().loc + frame.to_f64().size.to_point().downscale(2.0);
            let about = |p: Point<f64, Logical>| middle + (p - middle).upscale(grow);
            let frame_now =
                Rectangle::new(about(frame.to_f64().loc), frame.to_f64().size.upscale(grow));
            let decorated = self.decorated(window);
            if decorated && focused.as_ref() == Some(window) {
                let ring = ring_elements(
                    renderer,
                    &self.textures,
                    &self.style,
                    frame_now,
                    scale,
                    alpha,
                );
                elements.extend(ring.into_iter().map(SceneElement::Frame));
            }
            let radius = if decorated {
                (self.style.radius * scale).round() as i32
            } else {
                0
            };
            let clip = frame.to_f64().to_physical_precise_round(scale);
            let origin: Point<i32, _> =
                (location - geometry.loc - area.loc).to_physical_precise_round(scale);
            let opacity = managed.map_or(1.0, |m| m.opacity) * alpha;
            let content: Vec<WaylandSurfaceRenderElement<R>> =
                window.render_elements(renderer, origin, Scale::from(scale), opacity);
            // Drawn at its size, then scaled about its middle: an element's
            // size follows the output's scale, not the one it was made at.
            let centre = middle.to_physical_precise_round(scale);
            elements.extend(content.into_iter().map(|e| {
                let rounded = Rounded::new(e, clip, radius).at_scale(Scale::from(scale));
                if grow == 1.0 {
                    SceneElement::Window(rounded)
                } else {
                    SceneElement::Scaled(RescaleRenderElement::from_element(
                        rounded,
                        centre,
                        Scale::from(grow),
                    ))
                }
            }));
            if bar > 0
                && let Some(id) = id
            {
                elements.extend(
                    self.bar_element(renderer, id, rect.loc, scale)
                        .map(SceneElement::Frame),
                );
            }
            if decorated {
                let shadow = shadow_elements(
                    renderer,
                    &self.textures,
                    &self.style,
                    frame_now,
                    scale,
                    alpha,
                );
                elements.extend(shadow.into_iter().map(SceneElement::Frame));
            }
            if !veiled && focused.as_ref() == Some(window) {
                veil(&mut elements);
                veiled = true;
            }
        }
        if fullscreen {
            elements.extend(
                self.layer_elements(renderer, output, &[Layer::Top])
                    .into_iter()
                    .map(flat),
            );
        }
        elements.extend(
            self.layer_elements(renderer, output, &crate::handlers::layer_shell::BELOW)
                .into_iter()
                .map(flat),
        );
        // The wallpaper, at the very back: a third-party one (background
        // layer) covers it.
        elements.extend(
            self.wallpaper_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
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

impl State {
    /// Tells the clients shown on `output` that a frame went out, so they
    /// draw their next one: windows and their popups, layer surfaces, the
    /// lock client and the cursor. `states` says which surfaces the frame
    /// showed, when the backend knows; a surface is then only told by the
    /// output it shows most on.
    pub fn send_frames(
        &self,
        output: &Output,
        now: std::time::Duration,
        states: Option<&smithay::backend::renderer::element::RenderElementStates>,
    ) {
        use smithay::backend::renderer::element::default_primary_scanout_output_compare;
        use smithay::{
            desktop::{
                layer_map_for_output,
                utils::{
                    send_frames_surface_tree, surface_primary_scanout_output,
                    update_surface_primary_scanout_output,
                },
            },
            input::pointer::CursorImageStatus,
        };

        // No throttling: frames are paced by the backend.
        let throttle = Some(std::time::Duration::ZERO);
        let primary = |surface: &_, data: &_| -> Option<Output> {
            match states {
                Some(_) => surface_primary_scanout_output(surface, data),
                None => Some(output.clone()),
            }
        };
        for window in self.space.elements() {
            if let Some(states) = states {
                window.with_surfaces(|surface, data| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        data,
                        states,
                        default_primary_scanout_output_compare,
                    );
                });
            }
            window.send_frame(output, now, throttle, primary);
        }
        for layer in layer_map_for_output(output).layers() {
            layer.send_frame(output, now, throttle, |_, _| Some(output.clone()));
        }
        if let Some(lock) = self.lock_surface(output) {
            send_frames_surface_tree(lock, output, now, throttle, |_, _| Some(output.clone()));
        }
        if let CursorImageStatus::Surface(cursor) = &self.cursor {
            send_frames_surface_tree(cursor, output, now, throttle, |_, _| Some(output.clone()));
        }
    }
}
