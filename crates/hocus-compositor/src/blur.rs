//! Frosted glass (#37): what is behind the shell's panels, and behind the
//! windows a rule asks it for, blurred by GLES shaders.
//!
//! Before a GLES backend draws an output, each pane of glass, from the
//! back, has what is behind it drawn into a texture of its own through a
//! damage tracker (`paint_glass`). When nothing behind it changed, nothing
//! is drawn and the blur of the last time stays: a still scene is never
//! blurred again, and moving the pointer or a panel's own content changing
//! costs nothing here. Only when something behind did change, the dual
//! Kawase blur (Marius Bjørge, "Bandwidth-efficient rendering", SIGGRAPH
//! 2015) halves that picture a few times and doubles it back up, each step
//! averaging a few samples around each pixel (`shaders/blur_*.frag`). The
//! scene, whatever its renderer, shows the result under the pane, cut to
//! its rounded corners. Nothing is queued per frame: the blur follows the
//! damage of the frames that are drawn anyway.
//!
//! The pixman renderer has no shaders: there, and wherever the blur is off
//! (transparency off, high contrast, reduced motion, the power saver),
//! the theme has no
//! glass and the panels stay solid, the same pixels on every machine.

use std::{any::Any, collections::BTreeMap};

use hocus_core::WindowId;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Frame, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::{
                Id, Kind,
                texture::TextureRenderElement,
                utils::{Relocate, RelocateRenderElement},
            },
            gles::{GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType},
        },
    },
    output::Output,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

use crate::{State, decoration::Rounded, scene::Build};

/// A shell surface with panels on glass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Column,
    /// The panel's strip or top bar (#263).
    Bar,
    Popover,
    Toasts,
    Quick,
    Launcher,
    Dock,
    DesktopSwitcher,
    WindowSwitcher,
    Authorize,
    Leave,
    Share,
    Welcome,
    Circle,
}

/// What has glass behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pane {
    /// A window a rule asks it for.
    Window(WindowId),
    /// A panel of a shell surface, by its order there.
    Shell(Surface, usize),
}

/// A pane of glass in an output's scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub pane: Pane,
    /// Output-local, logical.
    pub rect: Rectangle<f64, Logical>,
    /// Its corners' radius, logical.
    pub radius: f64,
    /// How opaque the glass is drawn: the pane's own fading.
    pub alpha: f32,
}

/// The panels of a shell surface on glass, `origin` its corner on the
/// output, logical.
pub fn spots(
    surface: Surface,
    origin: Point<f64, Logical>,
    glass: &[hocus_ui::Glass],
) -> Vec<Spot> {
    glass
        .iter()
        .enumerate()
        .map(|(i, g)| Spot {
            pane: Pane::Shell(surface, i),
            rect: Rectangle::new(
                origin + Point::from((f64::from(g.rect.x), f64::from(g.rect.y))),
                Size::from((f64::from(g.rect.w), f64::from(g.rect.h))),
            ),
            radius: f64::from(g.radius),
            alpha: g.opacity,
        })
        .collect()
}

/// How many times the picture is halved, and how far apart the samples
/// are, for a blur that reaches `reach` pixels: each level doubles the
/// reach; the offset makes up the rest, between 1 and 2.
pub fn levels(reach: f64) -> (usize, f32) {
    let reach = reach.max(4.0);
    let n = ((reach / 2.0).log2().floor() as usize).clamp(1, 6);
    let offset = reach / 2f64.powi(n as i32 + 1);
    (n, offset.clamp(1.0, 2.0) as f32)
}

/// The shaders, once compiled, or why there are none.
#[derive(Debug, Default)]
enum Shaders {
    /// No GLES backend has painted yet.
    #[default]
    Unknown,
    Ready(Box<Programs>),
    /// They did not compile: no glass.
    Broken,
}

#[derive(Debug)]
struct Programs {
    down: GlesTexProgram,
    up: GlesTexProgram,
}

impl Programs {
    fn compile(renderer: &mut GlesRenderer) -> Result<Self, String> {
        let uniforms = [
            UniformName::new("half_pixel", UniformType::_2f),
            UniformName::new("half_source", UniformType::_2f),
            UniformName::new("offset", UniformType::_1f),
        ];
        let down = renderer
            .compile_custom_texture_shader(include_str!("shaders/blur_down.frag"), &uniforms)
            .map_err(|e| format!("down: {e}"))?;
        let up = renderer
            .compile_custom_texture_shader(include_str!("shaders/blur_up.frag"), &uniforms)
            .map_err(|e| format!("up: {e}"))?;
        Ok(Self { down, up })
    }
}

/// One pane's glass on one output: what is behind it, and its blur.
#[derive(Debug)]
struct Frosted {
    /// What was drawn behind: the pane and the blur's reach around it,
    /// output-local, physical.
    region: Rectangle<i32, Physical>,
    scale: f64,
    /// How far the blur reaches, physical pixels.
    reach: f64,
    tracker: OutputDamageTracker,
    behind: GlesTexture,
    /// Halves of `behind`; the first holds the blur once done.
    levels: Vec<GlesTexture>,
    offset: f32,
    /// A new id each time the blur is done: the outputs draw it again.
    id: Id,
    blurred: bool,
}

impl Frosted {
    fn new(
        renderer: &mut GlesRenderer,
        region: Rectangle<i32, Physical>,
        scale: f64,
        reach: f64,
    ) -> Result<Self, String> {
        let (n, offset) = levels(reach);
        let size = |w: i32, h: i32| Size::<i32, Buffer>::from((w.max(1), h.max(1)));
        let behind: GlesTexture = renderer
            .create_buffer(Fourcc::Abgr8888, size(region.size.w, region.size.h))
            .map_err(|e| e.to_string())?;
        let mut levels = Vec::with_capacity(n);
        let (mut w, mut h) = (region.size.w, region.size.h);
        for _ in 0..n {
            (w, h) = ((w + 1) / 2, (h + 1) / 2);
            let level: GlesTexture = renderer
                .create_buffer(Fourcc::Abgr8888, size(w, h))
                .map_err(|e| e.to_string())?;
            levels.push(level);
        }
        Ok(Self {
            region,
            scale,
            reach,
            tracker: OutputDamageTracker::new(region.size, scale, Transform::Normal),
            behind,
            levels,
            offset,
            id: Id::new(),
            blurred: false,
        })
    }

    /// Halves and doubles back what is behind into the first level.
    fn blur(&mut self, renderer: &mut GlesRenderer, programs: &Programs) -> Result<(), String> {
        let mut source = self.behind.clone();
        for level in &mut self.levels {
            pass(renderer, &programs.down, &source, level, self.offset)?;
            source = level.clone();
        }
        for i in (0..self.levels.len().saturating_sub(1)).rev() {
            let from = self.levels[i + 1].clone();
            pass(
                renderer,
                &programs.up,
                &from,
                &mut self.levels[i],
                self.offset,
            )?;
        }
        Ok(())
    }
}

/// Draws `from` over the whole of `to` with one of the blur's programs.
fn pass(
    renderer: &mut GlesRenderer,
    program: &GlesTexProgram,
    from: &GlesTexture,
    to: &mut GlesTexture,
    offset: f32,
) -> Result<(), String> {
    let size = to.size();
    let source = from.size();
    let target_size = Size::<i32, Physical>::from((size.w, size.h));
    let dest = Rectangle::from_size(target_size);
    let half = |s: Size<i32, Buffer>| (0.5 / s.w.max(1) as f32, 0.5 / s.h.max(1) as f32);
    let mut target = renderer.bind(to).map_err(|e| e.to_string())?;
    let mut frame = renderer
        .render(&mut target, target_size, Transform::Normal)
        .map_err(|e| e.to_string())?;
    frame
        .clear(Color32F::TRANSPARENT, &[dest])
        .map_err(|e| e.to_string())?;
    frame
        .render_texture_from_to(
            from,
            Rectangle::from_size(source.to_f64()),
            dest,
            &[dest],
            &[],
            Transform::Normal,
            1.0,
            Some(program),
            &[
                Uniform::new("half_pixel", half(size)),
                Uniform::new("half_source", half(source)),
                Uniform::new("offset", offset),
            ],
        )
        .map_err(|e| e.to_string())?;
    // The same context draws the output next, in order: no need to wait
    // for this pass (which would hold the loop).
    let _done = frame.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct Blurs {
    shaders: Shaders,
    /// By output name and pane.
    frosted: BTreeMap<(String, Pane), Frosted>,
}

impl State {
    /// Whether panels would be on glass where the renderer can blur: the
    /// shell is there, transparency is asked, and neither high contrast,
    /// reduced motion (what moves behind glass moves through it) nor the
    /// power saver says otherwise.
    fn glass_wanted(&self) -> bool {
        let a = &self.config.appearance;
        self.shell.is_some()
            && a.transparency
            && !a.high_contrast
            && !a.reduced_motion
            && !self.power_saver()
    }

    /// Whether panels are on glass now: wanted, and shaders to blur with.
    pub(crate) fn glass_on(&self) -> bool {
        matches!(self.blurs.shaders, Shaders::Ready(_)) && self.glass_wanted()
    }

    /// The pane of glass behind `window`, when a rule asks for it.
    pub(crate) fn window_glass(
        &self,
        id: WindowId,
        window: &smithay::desktop::Window,
        frame: Rectangle<f64, Logical>,
        radius: f64,
        alpha: f32,
    ) -> Option<Spot> {
        if !self.glass_on() {
            return None;
        }
        let (title, app_id) = crate::desktops::title_and_app_id(window);
        let rule = self.config.rule_for(
            app_id.as_deref().unwrap_or(""),
            title.as_deref().unwrap_or(""),
        );
        (rule.blur == Some(true)).then_some(Spot {
            pane: Pane::Window(id),
            rect: frame,
            radius,
            alpha,
        })
    }

    /// Whether something on `output` may be on glass: a shell surface's
    /// panels, or a window a rule asks it for.
    fn glass_may_show(&self, output: &Output) -> bool {
        let (popover, column) = self.shell_panes(output);
        self.config.rules.iter().any(|r| r.blur == Some(true))
            || !popover.glass.is_empty()
            || !column.glass.is_empty()
            || !self.panel_panes(output).glass.is_empty()
            || !self.quick_panes().glass.is_empty()
            || !self.launcher_panes(output).glass.is_empty()
            || !self.toasts_panes(output).glass.is_empty()
    }

    /// How far the blur reaches, logical pixels.
    fn blur_reach(&self) -> f64 {
        self.applied_theme
            .as_ref()
            .map_or(hocus_ui::theme::Effects::default().blur, |t| t.effects.blur)
            .into()
    }

    /// Before a GLES backend draws `output`: what is behind each pane of
    /// glass, blurred again where it changed.
    pub fn paint_glass(&mut self, renderer: &mut GlesRenderer, output: &Output) {
        let name = output.name();
        if !self.glass_wanted() {
            self.blurs.frosted.retain(|(o, _), _| *o != name);
            return;
        }
        if matches!(self.blurs.shaders, Shaders::Unknown) {
            self.blurs.shaders = match Programs::compile(renderer) {
                Ok(programs) => Shaders::Ready(Box::new(programs)),
                Err(err) => {
                    tracing::warn!(%err, "panels are solid: no blur shaders");
                    Shaders::Broken
                }
            };
            // The theme turns glass on.
            self.redraw.all();
        }
        if !matches!(self.blurs.shaders, Shaders::Ready(_)) {
            return;
        }
        // Nothing on glass there (a screen without panels): no scene to
        // build twice.
        if !self.glass_may_show(output) {
            self.blurs.frosted.retain(|(o, _), _| *o != name);
            return;
        }
        let mut build = Build::Panes(Vec::new());
        let _ = self.scene_build(renderer, output, false, &mut build);
        let Build::Panes(panes) = build else {
            return;
        };
        let seen: Vec<Pane> = panes.iter().map(|s| s.pane).collect();
        self.blurs
            .frosted
            .retain(|(o, pane), _| *o != name || seen.contains(pane));
        // From the back: a pane in front sees the glass behind it done.
        for spot in panes.iter().rev() {
            if let Err(err) = self.frost(renderer, output, spot) {
                tracing::warn!(output = name, ?spot.pane, %err, "could not blur");
                self.blurs.frosted.remove(&(name.clone(), spot.pane));
            }
        }
    }

    fn frost(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        spot: &Spot,
    ) -> Result<(), String> {
        let Some(mode) = output.current_mode() else {
            return Ok(());
        };
        let name = output.name();
        let scale = output.current_scale().fractional_scale();
        let screen = Rectangle::<i32, Physical>::from_size(
            output.current_transform().transform_size(mode.size),
        );
        let reach = self.blur_reach() * scale;
        let pane = spot.rect.to_physical_precise_round(scale);
        let r = reach.ceil() as i32;
        let region = Rectangle::new(
            pane.loc - Point::from((r, r)),
            pane.size + Size::from((2 * r, 2 * r)),
        )
        .intersection(screen)
        .filter(|r| !r.is_empty());
        let Some(region) = region else {
            self.blurs.frosted.remove(&(name, spot.pane));
            return Ok(());
        };
        let key = (name.clone(), spot.pane);
        let mut frosted = match self.blurs.frosted.remove(&key) {
            Some(f) if f.region.size == region.size && f.scale == scale && f.reach == reach => f,
            _ => Frosted::new(renderer, region, scale, reach)?,
        };
        if frosted.region != region {
            // Somewhere else: everything behind is drawn again.
            frosted.region = region;
            frosted.tracker = OutputDamageTracker::new(region.size, scale, Transform::Normal);
        }
        let mut build = Build::Behind {
            pane: spot.pane,
            found: false,
        };
        let behind = self.scene_build(renderer, output, false, &mut build);
        if !matches!(build, Build::Behind { found: true, .. }) {
            return Ok(());
        }
        let shift = Point::from((-region.loc.x, -region.loc.y));
        let behind: Vec<_> = behind
            .into_iter()
            .map(|e| RelocateRenderElement::from_element(e, shift, Relocate::Relative))
            .collect();
        let changed = {
            let mut target = renderer
                .bind(&mut frosted.behind)
                .map_err(|e| e.to_string())?;
            // The texture keeps its pixels: only what changed is drawn.
            let result = frosted
                .tracker
                .render_output(renderer, &mut target, 1, &behind, crate::BACKGROUND)
                .map_err(|e| format!("{e:?}"))?;
            result.damage.is_some_and(|d| !d.is_empty())
        };
        if changed || !frosted.blurred {
            let Shaders::Ready(programs) = &self.blurs.shaders else {
                return Ok(());
            };
            frosted.blur(renderer, programs)?;
            frosted.blurred = true;
            frosted.id = Id::new();
            self.measures.blur(&name);
        }
        self.blurs.frosted.insert(key, frosted);
        Ok(())
    }

    /// `spot`'s glass as blurred last, under its pane and cut to its
    /// corners, for any renderer whose textures are the painter's.
    pub(crate) fn glass_element<R>(
        &self,
        renderer: &R,
        output: &Output,
        spot: &Spot,
    ) -> Option<Rounded<TextureRenderElement<R::TextureId>>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        let frosted = self.blurs.frosted.get(&(output.name(), spot.pane))?;
        let level = frosted.levels.first().filter(|_| frosted.blurred)?;
        let texture = (level as &dyn Any).downcast_ref::<R::TextureId>()?;
        let scale = frosted.scale;
        let pane = spot.rect.to_physical_precise_round(scale);
        let inside = pane.intersection(frosted.region)?;
        // The pane's part of the region, in the blur's pixels.
        let (kx, ky) = (
            f64::from(level.size().w) / f64::from(frosted.region.size.w),
            f64::from(level.size().h) / f64::from(frosted.region.size.h),
        );
        let src = Rectangle::<f64, Logical>::new(
            (
                f64::from(inside.loc.x - frosted.region.loc.x) * kx,
                f64::from(inside.loc.y - frosted.region.loc.y) * ky,
            )
                .into(),
            (f64::from(inside.size.w) * kx, f64::from(inside.size.h) * ky).into(),
        );
        let size = inside.size.to_f64().to_logical(scale).to_i32_round();
        let element = TextureRenderElement::from_static_texture(
            frosted.id.clone(),
            renderer.context_id(),
            inside.loc.to_f64(),
            texture.clone(),
            1,
            Transform::Normal,
            (spot.alpha < 1.0).then_some(spot.alpha),
            Some(src),
            Some(size),
            None,
            Kind::Unspecified,
        );
        let radius = (spot.radius * scale).round() as i32;
        Some(Rounded::new(element, pane, radius).at_scale(Scale::from(scale)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blur_reaches_as_far_as_asked() {
        for reach in [8.0, 24.0, 32.0, 64.0, 100.0] {
            let (n, offset) = levels(reach);
            let got = f64::from(offset) * 2f64.powi(n as i32 + 1);
            assert!((1.0..=2.0).contains(&offset), "{reach}: {offset}");
            assert!(
                (got - reach).abs() <= reach * 0.5,
                "{reach}: {n} levels, {offset}"
            );
        }
        // Small or none: still one level.
        assert_eq!(levels(0.0).0, 1);
        assert_eq!(levels(32.0), (4, 1.0));
    }

    #[test]
    fn a_surface_s_panels_are_placed_on_the_output() {
        let glass = [hocus_ui::Glass {
            rect: hocus_ui::Rect::new(16.0, 20.0, 300.0, 400.0),
            radius: 24.0,
            opacity: 0.5,
        }];
        let s = spots(Surface::Quick, Point::from((100.0, 10.0)), &glass);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].pane, Pane::Shell(Surface::Quick, 0));
        assert_eq!(
            s[0].rect,
            Rectangle::new((116.0, 30.0).into(), (300.0, 400.0).into())
        );
        assert_eq!(s[0].alpha, 0.5);
    }
}
