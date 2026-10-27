//! What an output shows, front to back: the cursor, then each window from
//! the top with its focus ring, its content with rounded corners, and its
//! shadow. The headless and nested backends draw it, and screenshots too.

use hocus_core::Zone;
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, RendererSuper, Texture,
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement,
            texture::TextureRenderElement,
            utils::{Relocate, RelocateRenderElement, RescaleRenderElement},
        },
    },
    desktop::Window,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use crate::{
    State,
    blur::{Pane, Spot, Surface, spots},
    cursor::CursorElement,
    decoration::{
        Caster, Rounded, ShadowPiece, caster_elements, casters, hairline_elements, ring_elements,
        ring_elements_in, shadow_elements,
    },
    desktops::{model_id, rect},
    handlers::layer_shell::LayerKind as Layer,
    matrix_layers::LayerElement,
    window_mode::WindowMode,
};

render_elements! {
    pub SceneElement<R> where R: ImportAll + ImportMem;
    Cursor=CursorElement<R>,
    Window=Rounded<WaylandSurfaceRenderElement<R>>,
    /// A window drawn larger or smaller than it is: opening (#33).
    Scaled=RescaleRenderElement<Rounded<WaylandSurfaceRenderElement<R>>>,
    /// A window between two places, stretched: maximizing (#34).
    Morphed=RelocateRenderElement<RescaleRenderElement<Rounded<WaylandSurfaceRenderElement<R>>>>,
    /// A window's picture: closing, minimizing, maximizing (#34); the
    /// blur behind a pane of glass (#37).
    Snapshot=Rounded<TextureRenderElement<<R as RendererSuper>::TextureId>>,
    Frame=MemoryRenderBufferRenderElement<R>,
    /// A piece of a window's shadow, outside the window (#261).
    Shadow=ShadowPiece<R>,
    Solid=SolidColorRenderElement,
    /// A plain colour over a window's picture, its corners cut (#154).
    Tint=Rounded<SolidColorRenderElement>,
    /// A wallpaper that moves, painted by a shader (#108); the Matrix
    /// theme's scanlines and vignette (#169).
    Backdrop=TextureRenderElement<<R as RendererSuper>::TextureId>,
    /// What a brush of the Matrix theme drew (#221).
    Layer=LayerElement<R>,
}

/// What a scene is built for.
#[derive(Debug)]
pub(crate) enum Build {
    /// To be drawn.
    Draw,
    /// To list its panes of glass, front first (#37).
    Panes(Vec<Spot>),
    /// To draw what is behind one pane of glass: what is in front of it is
    /// left out; `found` once it is.
    Behind { pane: Pane, found: bool },
}

/// What a shell surface's panels need behind them: the blur of what is
/// behind each pane of glass (#37), and the shadows they cast (#281),
/// output-local.
#[derive(Debug, Default)]
pub(crate) struct Panes {
    pub glass: Vec<Spot>,
    pub shadows: Vec<Caster>,
}

impl Panes {
    /// The panes of a surface whose corner is at `origin` on the output.
    pub(crate) fn of(
        surface: Surface,
        origin: Point<f64, Logical>,
        glass: &[hocus_ui::Glass],
        shadows: &[hocus_ui::Cast],
    ) -> Self {
        Self {
            glass: spots(surface, origin, glass),
            shadows: casters(origin, shadows),
        }
    }
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
            // Dragged into a zone in the Matrix theme, its guides flash.
            self.matrix_guide(output, *zone, crate::matrix_windows::GuideKind::Flash);
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
        self.scene_build(renderer, output, cursor, &mut Build::Draw)
    }

    /// The column and its popover, front first, each on its glass with
    /// its shadows.
    fn column_elements<R>(
        &self,
        elements: &mut Vec<SceneElement<R>>,
        renderer: &mut R,
        output: &Output,
        build: &mut Build,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (popover, fading, column) = self.shell_elements(renderer, output);
        let (popover_panes, column_panes) = self.shell_panes(output);
        if let Some(popover) = popover {
            elements.push(SceneElement::Frame(popover));
            self.panes(elements, renderer, output, popover_panes, build);
        }
        if let Some(fading) = fading {
            elements.push(SceneElement::Frame(fading));
        }
        if let Some(column) = column {
            // Each agent's window, live, in its slot of the journal (#43).
            elements.extend(self.seat_slot_elements(renderer, output));
            elements.push(SceneElement::Frame(column));
            self.panes(elements, renderer, output, column_panes, build);
        }
    }

    /// What a shell surface's panels need behind them, the surface just
    /// pushed: their shadows, then the blur of what is behind them. Their
    /// own shadows are in front of their panes, so that the blur behind
    /// them leaves them out, as when the toolkit drew them.
    fn panes<R>(
        &self,
        elements: &mut Vec<SceneElement<R>>,
        renderer: &mut R,
        output: &Output,
        panes: Panes,
        build: &mut Build,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        elements.extend(self.cast_elements(renderer, output, &panes.shadows));
        self.glass(elements, renderer, output, panes.glass, build);
    }

    /// The shadows of shell panels (#281), output-local: as windows' are.
    pub(crate) fn cast_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        casters: &[Caster],
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = output.current_scale().fractional_scale();
        casters
            .iter()
            .flat_map(|c| caster_elements(renderer, &self.textures, &self.style, c, scale, 1.0))
            .map(SceneElement::Shadow)
            .collect()
    }

    /// The blur behind each of `spots` (#37), as `build` asks: listed,
    /// drawn, or where what is behind a pane starts.
    fn glass<R>(
        &self,
        elements: &mut Vec<SceneElement<R>>,
        renderer: &R,
        output: &Output,
        spots: Vec<Spot>,
        build: &mut Build,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        for spot in spots {
            match build {
                Build::Panes(panes) => panes.push(spot),
                Build::Behind { pane, found } if *pane == spot.pane => {
                    elements.clear();
                    *found = true;
                    continue;
                }
                Build::Behind { .. } | Build::Draw => {}
            }
            elements.extend(
                self.glass_element(renderer, output, &spot)
                    .map(SceneElement::Snapshot),
            );
        }
    }

    /// Everything `output` shows, front first, for `build`.
    pub(crate) fn scene_build<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        cursor: bool,
        build: &mut Build,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        self.ready_snapshots(renderer);
        self.matrix_layers.begin(&output.name());
        let mut elements = Vec::new();
        if cursor {
            elements.extend(
                self.cursor_elements(renderer, output)
                    .into_iter()
                    .map(SceneElement::Cursor),
            );
        }
        // MCP agents' pointers, rings and cards (#43), over everything
        // but the user's own pointer.
        elements.extend(self.seat_elements(renderer, output));
        // Drawing on the screen (#86), over everything.
        elements.extend(self.ink_elements(renderer, output));
        // The Matrix theme's scanlines and vignette over a fullscreen
        // overlay, on the GPU (#169).
        elements.extend(self.crt_elements(renderer, output));
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
            // The Matrix theme's padlock and rain behind it (#155).
            elements.extend(self.matrix_lock_elements(renderer, output));
            // Our lock fading in shows the desktop under it a moment.
            if !self.lock_entering() {
                return elements;
            }
        }
        // Unlocked, our lock screen parts from the centre.
        elements.extend(
            self.parting_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        elements.extend(self.matrix_unlock_elements(renderer, output));
        // The boot screen while the session is put back (#48), in the
        // Matrix theme with its rain (#155).
        elements.extend(
            self.boot_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        elements.extend(self.matrix_boot_elements(renderer, output));
        // What to share, asked by a portal.
        if let Some(panel) = self.share_elements(renderer, output) {
            elements.push(SceneElement::Frame(panel));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.share_panes(output),
                build,
            );
        }
        // The system's password dialog, over everything but the cursor.
        if let Some(panel) = self.auth_elements(renderer, output) {
            elements.push(SceneElement::Frame(panel));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.auth_panes(output),
                build,
            );
        }
        // Leaving: the countdown, over everything else.
        if let Some(panel) = self.leave_elements(renderer, output) {
            elements.push(SceneElement::Frame(panel));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.leave_panes(output),
                build,
            );
        }
        // The window menu at the pointer (#40).
        elements.extend(self.radial_elements(renderer, output));
        // A circled area, its menu and the answer (#46).
        if let Some(panel) = self.circle_elements(renderer, output) {
            elements.push(SceneElement::Frame(panel));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.circle_panes(output),
                build,
            );
        }
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
        if let Some(panel) = self.card_elements(renderer, output) {
            elements.push(SceneElement::Frame(panel));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.card_panes(output),
                build,
            );
        }
        // The curtain over everything but the cursor.
        elements.extend(
            self.curtain_elements(renderer, output)
                .into_iter()
                .map(SceneElement::Frame),
        );
        // The Matrix theme's guides, size readout, and the lights of
        // windows going into the dock or maximizing, over the panels (#154).
        elements.extend(self.matrix_overlay_elements(renderer, output));
        elements.extend(self.snapshot_lights(renderer, output));
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
        // under a fullscreen one. Each panel over the blur of what is
        // behind it (#37).
        let alt_tab = self.alt_tab_elements(renderer, output);
        if !alt_tab.is_empty() {
            elements.extend(alt_tab);
            self.panes(
                &mut elements,
                renderer,
                output,
                self.alt_tab_panes(output),
                build,
            );
        }
        let launcher = self.launcher_elements(renderer, output);
        if !launcher.is_empty() {
            elements.extend(launcher.into_iter().map(SceneElement::Frame));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.launcher_panes(output),
                build,
            );
        }
        // The Matrix theme's launcher: its parts coming or going, the dim
        // and the rain behind it (#155).
        elements.extend(self.matrix_launcher_on(renderer, output));
        let (sheet, dim) = self.quick_elements(renderer, output);
        if !sheet.is_empty() {
            elements.extend(sheet.into_iter().map(SceneElement::Frame));
            self.panes(&mut elements, renderer, output, self.quick_panes(), build);
        }
        elements.extend(dim.map(SceneElement::Solid));
        let over_toasts = self.panel_out_drawn();
        if over_toasts {
            self.column_elements(&mut elements, renderer, output, build);
        }
        elements.extend(self.matrix_toasts_on(renderer, output));
        let toasts = self.toast_elements(renderer, output);
        if !toasts.is_empty() {
            elements.extend(toasts.into_iter().map(SceneElement::Frame));
            self.panes(
                &mut elements,
                renderer,
                output,
                self.toasts_panes(output),
                build,
            );
        }
        // At rest, the column is beside the windows' toasts; out over the
        // windows (#263), it is over them too.
        if !over_toasts {
            self.column_elements(&mut elements, renderer, output, build);
        }
        // What stays of the panel, under the column that slides over it.
        let bar = self.panel_elements(renderer, output);
        if !bar.is_empty() {
            elements.extend(bar);
            self.panes(
                &mut elements,
                renderer,
                output,
                self.panel_panes(output),
                build,
            );
        }
        elements.extend(self.matrix_column_on(renderer, output));
        let switcher = self.switcher_elements(renderer, output);
        if !switcher.is_empty() {
            elements.extend(switcher);
            self.panes(
                &mut elements,
                renderer,
                output,
                self.switcher_panes(output),
                build,
            );
        }
        let dock = self.dock_elements(renderer, output);
        if !dock.is_empty() {
            elements.extend(dock);
            self.panes(
                &mut elements,
                renderer,
                output,
                self.dock_panes(output),
                build,
            );
        }
        elements.extend(self.panda_elements(renderer, output));
        // Third-party bars above the windows, but under a fullscreen one.
        if !fullscreen {
            elements.extend(
                self.layer_elements(renderer, output, &[Layer::Top])
                    .into_iter()
                    .map(flat),
            );
        }
        // Windows closing and minimizing (#34), over the others.
        elements.extend(self.snapshot_elements(renderer, output));
        // The Matrix theme's desktop left behind, raining away (#155).
        elements.extend(self.matrix_desktop_elements(renderer, output));
        let focused = self.focused_window();
        // Switching desktops (#36): the shown desktop's windows move with
        // the strip, and those of the desktops passing by are drawn where
        // they wait, below them.
        let slide = self.slide_view(output);
        let mut windows: Vec<(Window, Point<i32, Logical>)> = slide
            .map(|v| self.slide_extras(output, v))
            .unwrap_or_default();
        windows.extend(self.space.elements().filter_map(|w| {
            let mut at = self.space.element_location(w)?;
            if let Some(v) = slide
                && self.slides_with(w, output)
            {
                at.x += v.offset(v.shown);
            }
            Some((w.clone(), at))
        }));
        // The card at start stands alone on its screen: windows that open
        // while it asks (autostart, a program the session starts) wait
        // unseen behind it, else framed and ringed under its dim they
        // read as a second card (#262).
        if self.card_on(output) {
            windows.clear();
        }
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
        let focused_here = focused.as_ref().is_some_and(|f| {
            windows.iter().any(|(w, _)| w == f) && self.output_of(f) == Some(output)
        });
        if !veiled && !focused_here {
            veil(&mut elements);
            veiled = true;
        }
        for (window, mut location) in windows.into_iter().rev() {
            let window = &window;
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
            // Coming back from the dock: its magic lamp shows it.
            if id.is_some_and(|id| self.genie_hides(id)) {
                continue;
            }
            // Dragged or just let go: it bends like jelly (#35).
            if let Some(id) = id.filter(|id| self.wobbles(*id)) {
                let global = Rectangle::new(location, geometry.size).to_f64();
                let opacity = self.windows.get(&id).map_or(1.0, |m| m.opacity);
                elements
                    .extend(self.wobbly_elements(renderer, id, window, global, output, opacity));
                continue;
            }
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
            // Maximizing (#34): drawn between its old place and its new one,
            // output-local, its old picture fading over it.
            let morph = id.and_then(|id| {
                let live = Rectangle::new(location, geometry.size).to_f64();
                let view = self.morph_view(id, live)?;
                let local =
                    |r: Rectangle<f64, Logical>| Rectangle::new(r.loc - area.loc.to_f64(), r.size);
                Some((id, local(view.window), local(view.picture), view.alpha))
            });
            let frame_now = match morph {
                Some((_, r, _, _)) => Rectangle::new(
                    (r.loc.x, r.loc.y - f64::from(bar)).into(),
                    (r.size.w, r.size.h + f64::from(bar)).into(),
                ),
                None => frame_now,
            };
            let decorated = self.decorated(window);
            // Resized in the Matrix theme: a glow, then a flash (#154).
            let glow = id.and_then(|id| self.matrix_ring(id));
            if decorated && let Some((rgb, a)) = glow {
                let ring = ring_elements_in(
                    renderer,
                    &self.textures,
                    &self.style,
                    rgb,
                    frame_now,
                    scale,
                    alpha * a,
                );
                elements.extend(ring.into_iter().map(SceneElement::Frame));
            }
            let resizing = glow.is_some_and(|(_, a)| a >= 1.0);
            if decorated && focused.as_ref() == Some(window) && !resizing {
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
            // The Matrix theme's glyphs over a window materializing.
            if let Some(open) = managed.and_then(|m| m.motion.matrix(self.clock.now())) {
                elements.extend(self.matrix_open_elements(renderer, open, frame_now, scale));
            }
            // The hairline inside its edge, over what it shows.
            if decorated {
                let hairline = hairline_elements(
                    renderer,
                    &self.textures,
                    &self.style,
                    frame_now,
                    scale,
                    opacity,
                );
                elements.extend(hairline.into_iter().map(SceneElement::Frame));
            }
            if let Some((id, _, r, fade)) = morph {
                let clip_now = r.to_physical_precise_round(scale);
                elements.extend(
                    self.morph_picture(renderer, id, r, fade * opacity, scale)
                        .into_iter()
                        .map(|e| SceneElement::Snapshot(Rounded::new(e, clip_now, radius))),
                );
            }
            elements.extend(content.into_iter().map(|e| {
                let rounded = Rounded::new(e, clip, radius).at_scale(Scale::from(scale));
                if let Some((_, r, _, _)) = morph {
                    let live = rect.to_f64();
                    let k = Scale {
                        x: r.size.w / live.size.w.max(1.0),
                        y: r.size.h / live.size.h.max(1.0),
                    };
                    let shift = (r.loc - live.loc).to_physical_precise_round(scale);
                    SceneElement::Morphed(RelocateRenderElement::from_element(
                        RescaleRenderElement::from_element(
                            rounded,
                            rect.loc.to_physical_precise_round(scale),
                            k,
                        ),
                        shift,
                        Relocate::Relative,
                    ))
                } else if grow == 1.0 {
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
                    self.bar_element(renderer, id, rect.loc, scale, opacity)
                        .map(SceneElement::Frame),
                );
            }
            // A see-through window over the blur of what is behind it, when
            // a rule asks (#37).
            if let Some(spot) = id.and_then(|id| {
                let radius = if decorated { self.style.radius } else { 0.0 };
                self.window_glass(id, window, frame_now, radius, alpha)
            }) {
                self.glass(&mut elements, renderer, output, vec![spot], build);
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
                elements.extend(shadow.into_iter().map(SceneElement::Shadow));
            }
            // Dragged in the Matrix theme: its phosphor trail, behind it.
            if let Some(id) = id {
                let radius = if decorated { self.style.radius } else { 0.0 };
                elements.extend(self.matrix_trail_elements(
                    renderer,
                    id,
                    window,
                    area.loc.to_f64(),
                    radius,
                    scale,
                ));
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
        // The wallpaper, at the very back, moving or still: a third-party
        // one (background layer) covers it.
        match self.backdrop_element(renderer, output) {
            Some(moving) => elements.push(SceneElement::Backdrop(moving)),
            None => elements.extend(
                self.wallpaper_elements(renderer, output)
                    .into_iter()
                    .map(SceneElement::Frame),
            ),
        }
        elements
    }

    /// Whether the compositor draws the window's corners, shadow and ring:
    /// in normal or snapped mode, when the window has no shadow of its own (server-side
    /// decorations, or a buffer no larger than the window).
    pub(crate) fn decorated(&self, window: &Window) -> bool {
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
