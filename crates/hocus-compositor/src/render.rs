//! What an output shows, front first: the cursor, the ink drawn on the
//! screen (`crate::ink`), the layer surfaces above
//! the windows, a fullscreen window, the snap preview, windows leaving,
//! the windows with their grips, rims and shadows (opening, settling and
//! wobbling as they go), the bar's own surfaces and its glass unless the
//! bar is in front (`crate::front`), and the layers under them; and what
//! clients are told once a frame is out.

use std::time::Duration;

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, RendererSuper, Texture,
        element::{
            AsRenderElements, Kind, RenderElementState, RenderElementStates,
            default_primary_scanout_output_compare,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            solid::SolidColorRenderElement,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            texture::TextureRenderElement,
            utils::{CropRenderElement, Relocate, RelocateRenderElement, RescaleRenderElement},
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
        self, FramePiece, Keyed, Outside, RimKind, Rounded, ShadowPiece, ShadowStyle, grip_element,
        handle_element, preview_elements, rim_elements, shadow_elements,
    },
    front::{Pick, in_the_bar},
    going::{ShaderElement, Shaders},
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
    pub OutputElements<R> where R: ImportAll + ImportMem + Bend + Shaders;
    Cursor = CursorElement<R>,
    /// Layer surfaces.
    Surface = WaylandSurfaceRenderElement<R>,
    /// The bar's surface drawn on the CPU, cut to its rounded corners.
    Bar = Rounded<WaylandSurfaceRenderElement<R>>,
    Window = WindowSurface<R>,
    Frame = FramePiece<R>,
    Shadow = ShadowPiece<R>,
    /// The last picture of a window leaving.
    Picture = Rounded<TextureRenderElement<<R as RendererSuper>::TextureId>>,
    /// A window wobbling.
    Bent = BentElement,
    /// A window's live thumbnail in Alt+Tab's panel.
    Thumbnail = Keyed<WindowSurface<R>>,
    /// The desktop's background; the tint behind the launcher (5.1, #518).
    Backdrop = MemoryRenderBufferRenderElement<R>,
    /// The compositor's own shaders: going to an agent's streak (4.3),
    /// the halo (4.4).
    Shader = ShaderElement,
    /// The launcher's surface, its panel growing in or shrinking out.
    Scaled = RescaleRenderElement<WaylandSurfaceRenderElement<R>>,
    /// The capture's darkening and its flash (5.8); a password manager's
    /// window in a capture.
    Solid = SolidColorRenderElement,
    /// The strokes and the laser drawn on a screen (docs/design/ink.md).
    Ink = crate::ink::InkElement<R>,
    /// A half of the lock opening (7.1).
    Part = Keyed<RelocateRenderElement<CropRenderElement<WaylandSurfaceRenderElement<R>>>>,
}

/// What an output's elements are drawn for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The screen; `cursor`: the compositor draws the cursor (nested, the
    /// host does).
    Screen { cursor: bool },
    /// A capture (5.8): without what the capture draws, its toolbar and
    /// the toasts, a password manager's windows black; `cursor`: the
    /// pointer asked in the image.
    Capture { cursor: bool },
    /// The desktop's last frame as the session locks (7.1): what the lock
    /// fades in over, drawn as a capture is, the lock not yet in it.
    Freeze,
}

impl Purpose {
    fn cursor(self) -> bool {
        match self {
            Self::Screen { cursor } | Self::Capture { cursor } => cursor,
            Self::Freeze => false,
        }
    }

    pub(crate) fn screen(self) -> bool {
        matches!(self, Self::Screen { .. })
    }
}

/// A password manager's window is black in a capture, as the clipboard's
/// history keeps none of its copies (5.6).
const PROTECTED: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// What an output shows, element by element.
pub type OutputElement<R> = OutputElements<R>;

/// A window drawn alone for a capture, and where it is (global).
pub(crate) type Alone<R> = (Vec<OutputElement<R>>, Rectangle<f64, Logical>);

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

/// `compare`, as a choice between two outputs for a surface's primary
/// scan-out output: the closure's signature, made to hold for every
/// lifetime.
fn scanout_compare<F>(compare: F) -> F
where
    F: for<'a> Fn(
        &'a Output,
        &'a RenderElementState,
        &'a Output,
        &'a RenderElementState,
    ) -> &'a Output,
{
    compare
}

/// How a window looks at a moment.
pub(crate) struct Look {
    /// Where it is drawn, global.
    pub visual: Rectangle<f64, Logical>,
    pub alpha: f32,
    focus: f32,
    /// Its rim's pulse: gone to from an agent's row (4.3), or opened
    /// without the keyboard.
    pulse: crate::going::Pulse,
    grip: f32,
    decorated: bool,
    shadow: ShadowStyle,
    own_shadow: bool,
    /// Its corners' radius as drawn, when not the window's own scaled.
    corner: Option<f64>,
}

impl Look {
    /// A window drawn small at `visual` (global), its corners `corner`
    /// round, without its frame.
    pub(crate) fn thumbnail(visual: Rectangle<f64, Logical>, alpha: f32, corner: f64) -> Self {
        Self {
            visual,
            alpha,
            focus: 0.0,
            pulse: crate::going::Pulse::default(),
            grip: 0.0,
            decorated: true,
            shadow: ShadowStyle::UNFOCUSED,
            own_shadow: true,
            corner: Some(corner),
        }
    }
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
            corner: None,
        })
    }

    /// The window's surfaces drawn into `look.visual`, output-local from
    /// `origin` (global), front first: popups first. A window out of sight
    /// is drawn from where it comes back.
    pub(crate) fn window_parts<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        look: &Look,
        origin: Point<f64, Logical>,
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        let window = &managed.window;
        let Some(home) = self.home_rect(id) else {
            return;
        };
        let loc = Point::<i32, Logical>::from((home.x, home.y));
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
        let radius = if let Some(corner) = look.corner {
            (corner * scale).round() as i32
        } else if look.decorated {
            (decoration::RADIUS * scale * k.x.min(k.y)).round() as i32
        } else {
            0
        };
        // A thumbnail shows its window's geometry and nothing around it.
        let outside = if look.corner.is_some() {
            Outside::Cropped
        } else {
            Outside::Shown
        };
        let wrap = |e: WaylandSurfaceRenderElement<R>, radius: i32| {
            let scaled = RescaleRenderElement::from_element(e, actual_loc, k);
            let moved = RelocateRenderElement::from_element(scaled, offset, Relocate::Relative);
            OutputElements::Window(Rounded::new(moved, clip, radius, outside, scale))
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
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
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
            // The ring in whole pixels: a texture for each of its widths,
            // not one for each frame.
            let ring = look.pulse.ring.map_or((1.0, 0.0), |(width, alpha)| {
                ((width * scale).round().max(1.0) / scale, alpha)
            });
            for (kind, width, alpha) in [
                (RimKind::Pulse, crate::going::RIM_WIDTH, look.pulse.rim),
                (RimKind::Ring, ring.0, ring.1),
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
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
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
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        self.elements_for(renderer, output, Purpose::Screen { cursor })
    }

    /// What `output` shows for `purpose`, front first.
    pub fn elements_for<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        purpose: Purpose,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        self.grabber.ready(renderer);
        let now = self.now();
        self.drawn_at.set(Some(now));
        let mut elements: Vec<OutputElement<R>> = Vec::new();
        let Some(area) = self.space.output_geometry(output) else {
            return elements;
        };
        let origin = area.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let sc = Scale::from(scale);
        if purpose.cursor() {
            elements.extend(
                self.cursor_elements(renderer, output)
                    .into_iter()
                    .map(OutputElements::Cursor),
            );
        }
        // Locked (7.1): the lock alone, whatever the purpose but its own
        // last look at the desktop.
        if purpose != Purpose::Freeze
            && let Some(mut lock) = self.lock_elements(renderer, output, purpose, now)
        {
            elements.append(&mut lock);
            return elements;
        }
        // Opening: its halves over everything (on the screen; a capture
        // sees the desktop it opens on).
        if purpose.screen() {
            let mut parting = self.lock_parting_elements(renderer, output, now);
            elements.append(&mut parting);
        }
        // The share's chooser over everything but the cursor, and the
        // ring around a screen shared (5.11): on the screen only.
        if purpose.screen() {
            self.share_elements(renderer, output, now, &mut elements);
            // The recorded area's frame (5.10).
            self.record_elements(output, &mut elements);
        }
        // The capture over everything but the cursor (5.8); its toolbar's
        // glass goes in once what is behind it is known.
        let capture_glass = if purpose.screen() {
            self.capture_elements(renderer, output, now, &mut elements)
        } else {
            None
        };
        // A capture's editor and its thumbnail (5.9), never in a capture.
        let editor_glass = if purpose.screen() {
            self.capture_after_elements(renderer, output, now, &mut elements)
        } else {
            None
        };
        // The surfaces of layer `which` that `pick` takes; where the bar's
        // first surface is drawn among `out`, if it is.
        let layers = |renderer: &mut R,
                      which: Layer,
                      pick: Pick,
                      out: &mut Vec<OutputElement<R>>| {
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
                        | hocus_link::PEEK_NAMESPACE
                        | hocus_link::LAUNCHER_NAMESPACE
                        | hocus_link::LAUNCHER_TOAST_NAMESPACE
                        | hocus_link::CAPTURE_TOOLBAR_NAMESPACE
                        | hocus_link::SHARE_CHOOSER_NAMESPACE
                        | hocus_link::CAPTURE_THUMB_NAMESPACE
                        | hocus_link::CAPTURE_EDITOR_NAMESPACE
                ) {
                    continue;
                }
                let Some(mut geometry) = map.layer_geometry(&layer) else {
                    continue;
                };
                // A popover moves in and out, and a notification's bubble;
                // they show from their first frame.
                // The emoji picker too (5.7).
                let popover = matches!(
                    layer.namespace(),
                    hocus_link::POPOVER_NAMESPACE | hocus_link::EMOJI_NAMESPACE
                );
                let look = if popover {
                    self.panel
                        .popovers
                        .look(&layer, now, self.popover_motion())
                        .map(|l| (l.alpha, l.shift, l.rise))
                } else if hocus_link::slides_in(layer.namespace()) {
                    self.bubbles
                        .look(&layer, now, self.bubbles_slide())
                        .map(|l| (l.alpha, l.shift, 0.0))
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
                    hocus_link::CLOCK_NAMESPACE
                    | hocus_link::LEVELS_NAMESPACE
                    | hocus_link::CONNECTIONS_NAMESPACE
                    | hocus_link::DESKTOPS_NAMESPACE
                    | hocus_link::AGENTS_NAMESPACE
                    | hocus_link::TASKS_NAMESPACE
                    | hocus_link::IN_USE_NAMESPACE
                    | hocus_link::MEETING_NAMESPACE
                    | hocus_link::TODAY_NAMESPACE
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
        };
        // The strokes and the laser over the windows and the shell's
        // surfaces, under what the compositor shows to share, record or
        // capture: in a share's, a recording's or a capture's frames too.
        self.ink_elements(renderer, output, now, &mut elements);
        // The desktops' dots and Alt+Tab, over everything.
        self.overlay_elements(renderer, output, now, &mut elements);
        // The launcher's toast over the launcher fading out; never in a
        // capture.
        if purpose.screen() {
            self.toast_elements(renderer, output, now, &mut elements);
        }
        // The launcher over the rest, the bar too; its glass goes right
        // under what darkens the desktop.
        let launcher = self
            .launcher_elements(renderer, output, now, &mut elements)
            .map(|alpha| (elements.len(), alpha));
        layers(renderer, Layer::Overlay, Pick::All, &mut elements);
        // The bar's own surfaces go under the windows unless the pointer
        // brought it in front (#519); its popovers, its halo, the peek and
        // the bubbles stay above them.
        let front = self.bar_in_front(output);
        // The windows on this output, front first, where their desktop is
        // on the strip; a fullscreen one on top goes over the bars.
        let looks: Vec<(WindowId, Look)> = self
            .model
            .stack()
            .iter()
            .rev()
            .copied()
            .filter(|id| !self.model.is_minimized(*id))
            .filter_map(|id| {
                let (offset, alpha) = self.strip_offset(id, now)?;
                let mut look = self.look(id, now)?;
                look.visual.loc.x += offset;
                look.alpha *= alpha;
                Some((id, look))
            })
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
        // The handle of a pair's seam, where the pair is now: right above
        // the front one of the two, so that a window over the seam covers
        // it (§8).
        let mut handle = self.seam_handle.and_then(|(seam, handle)| {
            let live = self.live_seam(seam.first, seam.second)?;
            let (x, y) = live.centre();
            area.to_f64()
                .contains((x, y))
                .then(|| (live, handle.alpha(now)))
        });
        let mut windows = Vec::new();
        for (id, look) in &looks {
            if let Some((seam, alpha)) = handle.take_if(|(s, _)| *id == s.first || *id == s.second)
            {
                let (x, y) = seam.centre();
                windows.extend(
                    handle_element(
                        renderer,
                        &self.textures,
                        &self.seam_handle_id,
                        Point::from((x, y)) - origin,
                        seam.vertical,
                        scale,
                        alpha,
                    )
                    .map(OutputElements::Frame),
                );
            }
            if !purpose.screen() && self.protected(*id) {
                let r = Rectangle::new(look.visual.loc - origin, look.visual.size);
                windows.push(OutputElements::Solid(SolidColorRenderElement::new(
                    // A capture's buffer is drawn whole, once: any id.
                    smithay::backend::renderer::element::Id::new(),
                    r.to_physical_precise_round(scale),
                    0,
                    PROTECTED,
                    Kind::Unspecified,
                )));
                continue;
            }
            // A shared window's ring, right over it (5.11).
            if purpose.screen() {
                let visual = Rectangle::new(look.visual.loc - origin, look.visual.size);
                windows.extend(self.window_ring(renderer, *id, &output.name(), visual, scale, now));
            }
            match self.bent_window(renderer, *id, look, origin, scale) {
                Some(bent) => windows.push(OutputElements::Bent(bent)),
                None => self.framed_window(renderer, *id, look, origin, scale, &mut windows),
            }
        }
        if fullscreen_on_top {
            elements.append(&mut windows);
        }
        // The halo over the bar's surfaces and around them, under the
        // popovers beside it (4.4); with the bar under the windows, over
        // them still.
        let bar_at = layers(renderer, Layer::Top, Pick::above(front), &mut elements);
        let halo_at = if front {
            bar_at
        } else {
            let map = layer_map_for_output(output);
            map.layers()
                .any(|l| in_the_bar(l.namespace()))
                .then_some(elements.len())
        };
        if let Some(at) = halo_at
            && let Some(bar) = self.bar_rect(output, now)
        {
            let halo = self.halo_elements(renderer, output, bar, now);
            elements.splice(at..at, halo.into_iter().map(OutputElements::Shader));
        }
        // The panda peeking out (4.5): over the windows, under the bar's
        // surfaces and its halo, out of whose edge it comes.
        self.peek_elements(renderer, output, now, &mut elements);
        // Going to an agent: the streak over the windows, under the bar.
        // Over its glass too, which it does not blur again at each of its
        // frames: it is transparent where it starts, on its row.
        if let Some(streak) = self.streak_at(now)
            && let Some(element) = renderer.streak_element(&streak, origin, scale)
        {
            elements.push(OutputElements::Shader(element));
        }
        // The popovers' glass goes right under the layers above the
        // windows, over everything after them; the bar's too when it is in
        // front.
        let glass_at = elements.len();
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
                    .map(|e| {
                        OutputElements::Picture(Rounded::new(
                            e,
                            clip,
                            radius,
                            Outside::Shown,
                            scale,
                        ))
                    }),
            );
        }
        elements.append(&mut windows);
        // Under the windows, the bar, its glass right under it showing
        // what is under the bar: a window over it hides both there.
        layers(renderer, Layer::Top, Pick::below(front), &mut elements);
        let bar_glass_at = if front { glass_at } else { elements.len() };
        layers(renderer, Layer::Bottom, Pick::All, &mut elements);
        layers(renderer, Layer::Background, Pick::All, &mut elements);
        if let Some(backdrop) = self.backdrop_element(renderer, output) {
            elements.push(backdrop);
        }
        let mut glass = self.glass_element(renderer, output, now, &elements[bar_glass_at..]);
        let popovers = self.popover_glass(renderer, output, now, &elements[glass_at..]);
        // From the back, so that the places before stay right.
        if bar_glass_at > glass_at
            && let Some(pane) = glass.take()
        {
            elements.insert(bar_glass_at, pane);
        }
        for pane in popovers.into_iter().chain(glass) {
            elements.insert(glass_at, pane);
        }
        if let Some((at, alpha)) = launcher
            && let Some(pane) = self.launcher_glass(renderer, output, alpha, &elements[at..])
        {
            elements.insert(at, pane);
        }
        // In front of the launcher's: inserted after it.
        if let Some((at, alpha)) = editor_glass
            && let Some(pane) = self.capture_editor_glass(renderer, output, alpha, &elements[at..])
        {
            elements.insert(at, pane);
        }
        if let Some(glass) = capture_glass
            && let Some(pane) = self.capture_glass(renderer, output, glass, &elements[glass.at..])
        {
            elements.insert(glass.at, pane);
        }
        elements
    }

    /// Window `id` alone, for a capture of it (5.8): its surfaces and its
    /// popups at its place, at `scale`, without its frame or its shadow;
    /// black for a password manager. Where it is (global).
    pub(crate) fn window_alone<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        scale: f64,
    ) -> Option<Alone<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let now = self.now();
        let rect = crate::windows::rectangle(self.rect_of(id)?).to_f64();
        let mut look = self.look(id, now)?;
        look.visual = rect;
        look.alpha = 1.0;
        let mut out = Vec::new();
        // Locked (7.1), a window shared or recorded shows black.
        if self.locked() || self.protected(id) {
            out.push(OutputElements::Solid(SolidColorRenderElement::new(
                smithay::backend::renderer::element::Id::new(),
                Rectangle::from_size(rect.size).to_physical_precise_round(scale),
                0,
                PROTECTED,
                Kind::Unspecified,
            )));
        } else {
            self.window_parts(renderer, id, &look, rect.loc, scale, &mut out);
        }
        Some((out, rect))
    }

    /// Whether window `id` is a password manager's: black in captures.
    fn protected(&self, id: WindowId) -> bool {
        self.windows
            .get(&id)
            .and_then(|m| crate::windows::app_id(&m.window))
            .is_some_and(|app| crate::clipboard::password_manager(&app))
    }

    /// The launcher on `output` at `now` (5.1), front first: its surface,
    /// its panel scaled about its middle, and the bar's tint over the
    /// desktop under it. How opaque it is, when it shows.
    fn launcher_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) -> Option<f32>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let layer = self.launcher_on(output)?;
        let look = self
            .panel
            .launchers
            .look(&layer, now, self.launcher_motion())
            .filter(|l| l.alpha > 0.0)?;
        let size = self.space.output_geometry(output)?.size;
        let geometry = layer_map_for_output(output).layer_geometry(&layer)?;
        let scale = output.current_scale().fractional_scale();
        let loc = geometry.loc.to_physical_precise_round(scale);
        let surfaces: Vec<WaylandSurfaceRenderElement<R>> = AsRenderElements::<R>::render_elements(
            &layer,
            renderer,
            loc,
            Scale::from(scale),
            look.alpha,
        );
        let (mx, my) = hocus_link::launcher_middle(f64::from(size.w), f64::from(size.h));
        let middle = Point::<f64, Logical>::from((mx, my)).to_physical_precise_round(scale);
        out.extend(surfaces.into_iter().map(|e| {
            OutputElements::Scaled(RescaleRenderElement::from_element(e, middle, look.scale))
        }));
        // The bar's tint, its column stretched over the whole screen.
        let tint = self.panel.launchers.tint();
        // Its whole column, given: with none, the gradient only covered
        // the screen's first 64 rows (seen on the Intel GPU).
        let column =
            Rectangle::<f64, Logical>::from_size((1.0, crate::launcher::TINT_ROWS as f64).into());
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            &tint,
            Some(look.alpha),
            Some(column),
            Some(size),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(OutputElements::Backdrop(element)),
            Err(err) => {
                tracing::warn!(output = output.name(), %err, "could not tint behind the launcher");
            }
        }
        Some(look.alpha)
    }

    /// The launcher's toasts on `output` at `now` (5.2), where their motion
    /// has them; on the CPU, whose buffers have no alpha, cut to their
    /// rounded corners.
    fn toast_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = output.current_scale().fractional_scale();
        let motion = self.launcher_motion();
        for layer in self.launcher_toasts_on(output) {
            let geometry = layer_map_for_output(output).layer_geometry(&layer);
            let (Some(look), Some(geometry)) = (
                self.panel.launchers.toast_look(&layer, now, motion),
                geometry,
            ) else {
                continue;
            };
            if look.alpha <= 0.0 {
                continue;
            }
            let mut at = geometry.loc.to_f64();
            at.y += look.drop;
            let loc: Point<i32, Physical> = at.to_physical_precise_round(scale);
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(
                    &layer,
                    renderer,
                    loc,
                    Scale::from(scale),
                    look.alpha,
                );
            if self.shell.software {
                let clip = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
                let radius = (crate::launcher::TOAST_RADIUS * scale).round() as i32;
                out.extend(surfaces.into_iter().map(|e| {
                    OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
                }));
            } else {
                out.extend(surfaces.into_iter().map(OutputElements::Surface));
            }
        }
    }

    /// The glass under the launcher on `output`, `alpha` opaque: what is
    /// `behind` it, the whole screen, blurred. None without glass.
    fn launcher_glass<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        alpha: f32,
        behind: &[OutputElement<R>],
    ) -> Option<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !self.glass_on() {
            return None;
        }
        let size = self.space.output_geometry(output)?.size;
        let scale = output.current_scale().fractional_scale();
        let region: Rectangle<i32, Physical> =
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale);
        let blurred = renderer.frost(
            &mut self.panes.borrow_mut(),
            &crate::glass::launcher_pane(&output.name()),
            region,
            scale,
            crate::glass::Frost::LAUNCHER,
            behind,
        )?;
        glass_piece(renderer, blurred, region, scale, alpha, 0.0)
    }

    /// The shell's peek surfaces on `output` at `now` (4.5), where their
    /// motion has them, cut at the bar's edge: under the bar, nothing of
    /// them shows. On the CPU, whose buffers have no alpha, cut to their
    /// card's rounded corners too.
    fn peek_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = output.current_scale().fractional_scale();
        let edge = self
            .bar_rect(output, now)
            .map_or(i32::MAX, |bar| (bar.loc.x * scale).round() as i32);
        let map = layer_map_for_output(output);
        for layer in map
            .layers()
            .filter(|l| l.namespace() == hocus_link::PEEK_NAMESPACE)
        {
            let (Some(look), Some(geometry)) =
                (self.peek_look(layer, now), map.layer_geometry(layer))
            else {
                continue;
            };
            let mut at = geometry.loc.to_f64();
            at.x += look.shift;
            let loc: Point<i32, Physical> = at.to_physical_precise_round(scale);
            let size = geometry.size.to_physical_precise_round(scale);
            let width = (edge - loc.x).min(size.w);
            if width <= 0 {
                continue;
            }
            let clip = Rectangle::new(loc, (width, size.h).into());
            let radius = if self.shell.software {
                (crate::peek::CARD_RADIUS * scale).round() as i32
            } else {
                0
            };
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(
                    layer,
                    renderer,
                    loc,
                    Scale::from(scale),
                    look.alpha,
                );
            out.extend(surfaces.into_iter().map(|e| {
                OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
            }));
        }
    }

    /// The desktop's background on `output`, stretched over it.
    fn backdrop_element<R>(&self, renderer: &mut R, output: &Output) -> Option<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if self.config.plain_background {
            return None;
        }
        let mode = output.current_mode()?;
        let size = output.current_transform().transform_size(mode.size);
        let logical = self.space.output_geometry(output)?.size;
        let buffer = self
            .backdrops
            .borrow_mut()
            .entry((size.w, size.h))
            .or_insert_with(|| crate::backdrop::picture((size.w, size.h)))
            .clone();
        // Its whole picture, stretched over the screen.
        let picture = Rectangle::<f64, Logical>::from_size(
            (
                f64::from((size.w / crate::backdrop::SHRINK).max(1)),
                f64::from((size.h / crate::backdrop::SHRINK).max(1)),
            )
                .into(),
        );
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            &buffer,
            None,
            Some(picture),
            Some(logical),
            Kind::Unspecified,
        ) {
            Ok(element) => Some(OutputElements::Backdrop(element)),
            Err(err) => {
                tracing::warn!(output = output.name(), %err, "could not draw the background");
                None
            }
        }
    }

    /// The glass under the bar on `output` at `now`: what is `behind` it,
    /// blurred, cut to the bar's rounded corners. None without a bar
    /// drawn there, or without glass.
    fn glass_element<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        behind: &[OutputElement<R>],
    ) -> Option<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !self.glass_on() {
            return None;
        }
        // Only under a bar the shell shows: no glass without its tint.
        let shown = layer_map_for_output(output)
            .layers()
            .any(|l| l.namespace() == hocus_link::NAMESPACE);
        if !shown {
            return None;
        }
        let bar = self.bar_rect(output, now)?;
        let size = self.space.output_geometry(output)?.size;
        let scale = output.current_scale().fractional_scale();
        // The full bar's place and the blur's reach around it, whatever the
        // form: a switch does not draw it again.
        let edge = f64::from(hocus_link::EDGE);
        let full = f64::from(hocus_link::Form::Full.width());
        let reach = crate::glass::REACH;
        let screen = Rectangle::<f64, Logical>::from_size(size.to_f64());
        let around = Rectangle::<f64, Logical>::new(
            (f64::from(size.w) - edge - full - reach, edge - reach).into(),
            (
                full + 2.0 * reach,
                f64::from(size.h) - 2.0 * edge + 2.0 * reach,
            )
                .into(),
        )
        .intersection(screen)?;
        let region: Rectangle<i32, Physical> = around.to_physical_precise_round(scale);
        let blurred = renderer.frost(
            &mut self.panes.borrow_mut(),
            &output.name(),
            region,
            scale,
            crate::glass::Frost::BAR,
            behind,
        )?;
        let pane: Rectangle<i32, Physical> = bar.to_physical_precise_round(scale);
        glass_piece(renderer, blurred, pane, scale, 1.0, hocus_link::RADIUS)
    }

    /// The glass under the shell's popovers on `output` at `now`, as under
    /// the bar, in a pane of its own: over where a popover is and where it
    /// slides from, so that its motion does not blur it again.
    fn popover_glass<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        behind: &[OutputElement<R>],
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !self.glass_on() {
            return Vec::new();
        }
        let Some(size) = self.space.output_geometry(output).map(|g| g.size) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let motion = self.popover_motion();
        let map = layer_map_for_output(output);
        let mut panes = Vec::new();
        for layer in map.layers() {
            let (Some(look), Some(shown), Some(geometry)) = (
                self.panel.popovers.look(layer, now, motion),
                self.panel.popovers.shown(layer),
                map.layer_geometry(layer),
            ) else {
                continue;
            };
            let rise = shown.reach();
            if look.alpha <= 0.0 {
                continue;
            }
            let reach = crate::glass::REACH;
            let rect = geometry.to_f64();
            let screen = Rectangle::<f64, Logical>::from_size(size.to_f64());
            let Some(around) = Rectangle::<f64, Logical>::new(
                (rect.loc.x - reach, rect.loc.y - reach - rise).into(),
                (
                    rect.size.w + hocus_link::POPOVER_SHIFT + 2.0 * reach,
                    rect.size.h + 2.0 * reach + rise,
                )
                    .into(),
            )
            .intersection(screen) else {
                continue;
            };
            let Some(blurred) = renderer.frost(
                &mut self.panes.borrow_mut(),
                &if layer.namespace() == hocus_link::EMOJI_NAMESPACE {
                    crate::glass::emoji_pane(&output.name())
                } else {
                    crate::glass::popover_pane(&output.name())
                },
                around.to_physical_precise_round(scale),
                scale,
                crate::glass::Frost::BAR,
                behind,
            ) else {
                continue;
            };
            let mut at = rect;
            at.loc.x += look.shift.round();
            at.loc.y -= look.rise.round();
            let pane: Rectangle<i32, Physical> = at.to_physical_precise_round(scale);
            panes.extend(glass_piece(
                renderer,
                blurred,
                pane,
                scale,
                look.alpha,
                shown.radius(),
            ));
        }
        panes
    }
}

/// The part of `blurred` over `pane` (physical, output-local), cut to
/// corners `radius` round (logical), `alpha` opaque.
pub(crate) fn glass_piece<R>(
    renderer: &mut R,
    blurred: crate::glass::Blurred<R::TextureId>,
    pane: Rectangle<i32, Physical>,
    scale: f64,
    alpha: f32,
    radius: f64,
) -> Option<OutputElement<R>>
where
    R: Renderer + ImportAll + ImportMem + Bend + Shaders,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let inside = pane.intersection(blurred.region)?;
    // The bar's part of the region, in the blur's pixels.
    let (kx, ky) = (
        f64::from(blurred.size.w) / f64::from(blurred.region.size.w),
        f64::from(blurred.size.h) / f64::from(blurred.region.size.h),
    );
    let src = Rectangle::<f64, Logical>::new(
        (
            f64::from(inside.loc.x - blurred.region.loc.x) * kx,
            f64::from(inside.loc.y - blurred.region.loc.y) * ky,
        )
            .into(),
        (f64::from(inside.size.w) * kx, f64::from(inside.size.h) * ky).into(),
    );
    let element = TextureRenderElement::from_static_texture(
        blurred.id,
        renderer.context_id(),
        inside.loc.to_f64(),
        blurred.texture,
        1,
        smithay::utils::Transform::Normal,
        (alpha < 1.0).then_some(alpha),
        Some(src),
        Some(inside.size.to_f64().to_logical(scale).to_i32_round()),
        None,
        Kind::Unspecified,
    );
    let radius = (radius * scale).round() as i32;
    Some(OutputElements::Picture(Rounded::new(
        element,
        pane,
        radius,
        Outside::Cropped,
        scale,
    )))
}

impl State {
    /// Tells the clients shown on `output` that a frame went out, so that
    /// they draw their next one. `states` says which surfaces it showed,
    /// when the backend knows: a surface is then told only by the output
    /// it shows most on.
    pub fn send_frames(&self, output: &Output, states: Option<&RenderElementStates>) {
        let now: Duration = self.clock.now().into();
        // Frames are paced by the backend, never throttled here.
        let throttle = Some(Duration::ZERO);
        // A screen unplugged lives on a moment, its global disabled
        // (`crate::screens`): it is no surface's screen any more.
        let primary = |surface: &_, data: &_| -> Option<Output> {
            match states {
                Some(_) => {
                    surface_primary_scanout_output(surface, data).filter(|o| self.is_plugged(o))
                }
                None => Some(output.clone()),
            }
        };
        let compare = scanout_compare(|current, current_state, next, next_state| {
            if self.is_plugged(current) {
                default_primary_scanout_output_compare(current, current_state, next, next_state)
            } else {
                next
            }
        });
        // Locked (7.1), only the lock's surfaces draw: the others wait.
        let locked = self.locked();
        for window in self.space.elements().filter(|_| !locked) {
            window.with_surfaces(|surface, data| {
                if let Some(states) = states {
                    update_surface_primary_scanout_output(surface, output, data, states, compare);
                }
                tell_scale(surface, data, primary(surface, data).as_ref());
            });
            window.send_frame(output, now, throttle, primary);
        }
        for layer in layer_map_for_output(output).layers() {
            if !self.lock_frames_for(layer.wl_surface()) {
                continue;
            }
            layer.with_surfaces(|surface, data| tell_scale(surface, data, Some(output)));
            layer.send_frame(output, now, throttle, |_, _| Some(output.clone()));
        }
        if let Some(surface) = self.external_surface_on(output) {
            send_frames_surface_tree(&surface, output, now, throttle, |_, _| Some(output.clone()));
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
