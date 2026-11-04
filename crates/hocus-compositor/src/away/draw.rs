//! The curtain on a screen, front first: the shell's pill (on the screen
//! under the pointer as it came) and its shadow, then the dim over
//! everything else, the bar and its popovers included.
//!
//! - The dim goes from nothing to `rgba(4, 6, 10, 0.74)` along `p^1.15`
//!   over the curtain's 10 s, drawn at most ten times a second; with
//!   reduced motion it steps once a second (it says how long is left).
//! - The pill fades and rises 10 px in over 250 ms (out-cubic) from its
//!   first frame; at once with reduced motion or a shell on the CPU.
//! - Lifted, both fade out over 150 ms (`ease-in`), from where they were.
//! - Neither is ever in a capture, a share or a recording; the lock's
//!   picture of the desktop as it locks has them at their end, so that
//!   the screen never brightens between the curtain and the lock.

use std::time::Duration;

use hocus_link::{
    CURTAIN, CURTAIN_DIM, CURTAIN_DIM_ALPHA, CURTAIN_DIM_POWER, CURTAIN_FRAME, CURTAIN_LIFT,
    PILL_IN, PILL_RADIUS, PILL_RISE, ToShell,
};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Kind, solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::layer_map_for_output,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use super::Phase;
use crate::{
    State,
    decoration::{Outside, Rounded, shadow_elements},
    going::Shaders,
    render::{OutputElement, OutputElements, Purpose},
    wobble::Bend,
};

/// How the pill shows at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillLook {
    pub alpha: f32,
    /// How far under its place it still is, logical pixels.
    pub rise: f64,
}

impl PillLook {
    const GONE: Self = Self {
        alpha: 0.0,
        rise: 0.0,
    };
    const IN: Self = Self {
        alpha: 1.0,
        rise: 0.0,
    };
}

fn out_cubic(x: f64) -> f64 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

fn part(t: Duration, of: Duration) -> f64 {
    if of.is_zero() {
        return 1.0;
    }
    (t.as_secs_f64() / of.as_secs_f64()).clamp(0.0, 1.0)
}

/// The dim's opacity a curtain up for `t` has: a ramp, or a step a second
/// (`stepped`, reduced motion).
pub fn dim_at(t: Duration, stepped: bool) -> f32 {
    let t = if stepped {
        Duration::from_secs(t.as_secs())
    } else {
        t
    };
    (f64::from(CURTAIN_DIM_ALPHA) * part(t, CURTAIN).powf(CURTAIN_DIM_POWER)) as f32
}

/// How much of a lifted curtain is left after `t`: `ease-in` out.
fn left_after_lift(t: Duration) -> f32 {
    (1.0 - crate::motion::Bezier::EASE_IN.at(part(t, CURTAIN_LIFT))) as f32
}

impl State {
    /// Whether the pill moves in: no reduced motion, a shell on a GPU.
    fn pill_motion(&self) -> bool {
        self.popover_motion()
    }

    /// The dim's opacity and the pill's look at `now`, for a curtain up
    /// since `since`, its pill first drawn at `pill_at`.
    pub(crate) fn curtain_looks(
        &self,
        since: Duration,
        pill_at: Option<Duration>,
        now: Duration,
    ) -> (f32, PillLook) {
        let dim = dim_at(now.saturating_sub(since), self.reduced_motion());
        let pill = match pill_at {
            None => PillLook::GONE,
            Some(_) if !self.pill_motion() => PillLook::IN,
            Some(at) => {
                let p = out_cubic(part(now.saturating_sub(at), PILL_IN));
                PillLook {
                    alpha: p as f32,
                    rise: (1.0 - p) * PILL_RISE,
                }
            }
        };
        (dim, pill)
    }

    /// What the curtain draws on `output` for `purpose` at `now`, front
    /// first, onto `out`.
    pub(crate) fn curtain_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        purpose: Purpose,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let freeze = purpose == Purpose::Freeze;
        if !purpose.screen() && !freeze {
            return;
        }
        let (dim, pill, pill_output) = match (&self.away.phase, &self.away.lifted) {
            (
                Phase::Curtain {
                    since,
                    locks,
                    output,
                    pill_at,
                },
                _,
            ) => {
                let (dim, pill) = self.curtain_looks(*since, *pill_at, now);
                (dim, pill, locks.then_some(output.as_str()))
            }
            (_, Some(lifted)) if purpose.screen() => {
                let left = left_after_lift(now.saturating_sub(lifted.at));
                let pill = PillLook {
                    alpha: lifted.pill.alpha * left,
                    rise: lifted.pill.rise,
                };
                (lifted.dim * left, pill, lifted.pill_output.as_deref())
            }
            _ => match &self.away.under_lock {
                Some(ended) if freeze => (
                    CURTAIN_DIM_ALPHA,
                    PillLook::IN,
                    ended.pill_output.as_deref(),
                ),
                _ => return,
            },
        };
        let Some(size) = self.space.output_geometry(output).map(|g| g.size) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        if let Some(layer) = pill_output
            .filter(|_| pill.alpha > 0.0)
            .and_then(|name| self.curtain_layer(output, name))
        {
            let geometry = layer_map_for_output(output).layer_geometry(&layer);
            if let Some(mut geometry) = geometry {
                geometry.loc.y += pill.rise.round() as i32;
                let loc: Point<i32, Physical> = geometry.loc.to_physical_precise_round(scale);
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                    AsRenderElements::<R>::render_elements(
                        &layer,
                        renderer,
                        loc,
                        Scale::from(scale),
                        pill.alpha,
                    );
                if self.shell.software {
                    // The CPU's buffers have no alpha: cut to its corners.
                    let clip = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
                    let radius = (PILL_RADIUS * scale).round() as i32;
                    out.extend(surfaces.into_iter().map(|e| {
                        OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
                    }));
                } else {
                    out.extend(surfaces.into_iter().map(OutputElements::Surface));
                    let mut style = crate::popover::SHADOW;
                    style.radius = PILL_RADIUS;
                    out.extend(
                        shadow_elements(
                            renderer,
                            &self.textures,
                            &self.away.shadow,
                            style,
                            geometry.to_f64(),
                            scale,
                            pill.alpha,
                        )
                        .into_iter()
                        .map(OutputElements::Shadow),
                    );
                }
            }
        }
        if dim <= 0.0 {
            return;
        }
        let [r, g, b] = CURTAIN_DIM.map(|c| f32::from(c) / 255.0);
        let whole =
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale);
        out.push(OutputElements::Solid(SolidColorRenderElement::new(
            self.away.dim.clone(),
            whole,
            (dim * 1000.0) as usize,
            [r * dim, g * dim, b * dim, dim],
            Kind::Unspecified,
        )));
    }

    /// Called as motions are judged: whether the curtain moves now (its
    /// pill coming in, or all of it fading out once lifted); while it dims,
    /// its next frame (`wake`), a tenth of a second on (a second with
    /// reduced motion).
    pub(crate) fn curtain_tick(&mut self, now: Duration, wake: &mut impl FnMut(Duration)) -> bool {
        if let Phase::Curtain { since, pill_at, .. } = self.away.phase {
            if self.pill_motion() && pill_at.is_some_and(|at| now < at + PILL_IN) {
                return true;
            }
            let step = if self.reduced_motion() {
                Duration::from_secs(1)
            } else {
                CURTAIN_FRAME
            };
            let into = now.saturating_sub(since);
            let next =
                since + step * (u32::try_from(into.as_nanos() / step.as_nanos()).unwrap_or(0) + 1);
            if next < since + CURTAIN {
                wake(next);
            }
            return false;
        }
        if let Some(lifted) = &self.away.lifted {
            if now < lifted.at + CURTAIN_LIFT {
                return true;
            }
            self.lifted_gone();
        }
        false
    }

    /// The lifted curtain has faded out: the shell's pill goes.
    pub(crate) fn lifted_gone(&mut self) {
        if let Some(lifted) = self.away.lifted.take()
            && lifted.pill_output.is_some()
        {
            self.tell_shell(ToShell::CurtainEnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dim_ramps_along_its_power_or_steps_each_second() {
        let s = Duration::from_secs;
        assert_eq!(dim_at(s(0), false), 0.0);
        assert!((dim_at(s(10), false) - 0.74).abs() < 1e-6);
        assert!((dim_at(s(20), false) - 0.74).abs() < 1e-6);
        let half = dim_at(s(5), false);
        assert!((f64::from(half) - 0.74 * 0.5f64.powf(1.15)).abs() < 1e-6);
        // Stepped: the same over a second.
        assert_eq!(
            dim_at(Duration::from_millis(5900), true),
            dim_at(s(5), false)
        );
        assert!(dim_at(Duration::from_millis(900), true) == 0.0);
    }

    #[test]
    fn lifted_it_fades_out_in_150_ms() {
        assert_eq!(left_after_lift(Duration::ZERO), 1.0);
        assert!(left_after_lift(Duration::from_millis(50)) > 0.8);
        assert_eq!(left_after_lift(CURTAIN_LIFT), 0.0);
    }
}
