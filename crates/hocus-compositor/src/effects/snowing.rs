//! The snow on the desktop (#795): where it falls and settles, stepped as
//! the frames go ([`super::snow`] the simulation, `crate::render::snow`
//! its drawing).
//!
//! It snows on each screen that is on and shows no fullscreen window; the
//! caps follow their windows and the bars. A cap stays with its window
//! while the window is out of sight (on another desktop: it is carried
//! along), and goes as the window is minimised or closed. With reduced
//! motion the piles are drawn still and nothing falls; in the power saver,
//! while a screen is shared or recorded, nothing is drawn (the snow waits);
//! on the CPU there is none.

use std::time::Duration;

use smithay::utils::{Logical, Rectangle};

use super::{
    Still,
    snow::{Holder, Snow, shed_strength},
};
use crate::{State, model::Mode};

/// The bar's cap, inset from its sides as the board's.
const BAR_INSET: f64 = 6.0;

/// How the snow shows now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snowing {
    /// Falling and settling.
    Falling,
    /// Its piles drawn still (reduced motion).
    Still,
}

impl State {
    /// How the snow shows now; `None`: not at all.
    pub(crate) fn snowing(&self) -> Option<Snowing> {
        if !self.config.effects.snow {
            return None;
        }
        match self.effects_still() {
            None => Some(Snowing::Falling),
            Some(Still::ReducedMotion) => Some(Snowing::Still),
            Some(Still::PowerSaver | Still::Shown | Still::Cpu) => None,
        }
    }

    /// The edge of `holder`'s cap where it is drawn at `now` (global,
    /// logical), when it shows: a window's top as drawn (on the strip as
    /// it slides), a bar's.
    pub(crate) fn snow_edge(
        &self,
        holder: &Holder,
        now: Duration,
    ) -> Option<Rectangle<f64, Logical>> {
        match holder {
            Holder::Window(id) => {
                let id = *id;
                if self.model.is_minimized(id) || self.model.mode(id) == Mode::Fullscreen {
                    return None;
                }
                let (offset, _) = self.strip_offset(id, now)?;
                let mut edge = self.visual_rect(id, now)?;
                edge.loc.x += offset;
                Some(edge)
            }
            Holder::Bar(name) => {
                let output = self.space.outputs().find(|o| o.name() == *name)?;
                // Only on a bar the shell shows.
                smithay::desktop::layer_map_for_output(output)
                    .layers()
                    .any(|l| l.namespace() == hocus_link::NAMESPACE)
                    .then_some(())?;
                let area = self.space.output_geometry(output)?;
                let bar = self.bar_rect(output, now)?;
                Some(Rectangle::new(
                    (
                        f64::from(area.loc.x) + bar.loc.x + BAR_INSET,
                        f64::from(area.loc.y) + bar.loc.y,
                    )
                        .into(),
                    ((bar.size.w - 2.0 * BAR_INSET).max(0.0), bar.size.h).into(),
                ))
            }
        }
    }

    /// The snow moves on to `now` (#795): started as it is turned on, its
    /// piles grown; its caps following their edges; a window moved
    /// sharply shedding its snow; stepped while it falls. Whether it
    /// moves (a frame is wanted).
    pub(crate) fn tick_snow(&mut self, now: Duration) -> bool {
        let Some(snowing) = self.snowing() else {
            if self.config.effects.snow && self.fx.snow.is_some() {
                // Waiting (the power saver, a share): kept, not drawn.
                return false;
            }
            if self.fx.snow.take().is_some() {
                tracing::debug!("the snow stops");
                self.redraw = true;
            }
            self.fx.motions.clear();
            return false;
        };
        if self.fx.snow.is_none() {
            self.fx.snows += 1;
            tracing::debug!(?snowing, "it snows");
            self.fx.snow = Some(Snow::new(now, self.fx.snows));
            self.redraw = true;
        }
        // The windows shown, front first: their caps follow them.
        let shown: Vec<(Holder, Rectangle<f64, Logical>)> = self
            .model
            .stack()
            .iter()
            .rev()
            .map(|id| Holder::Window(*id))
            .chain(self.space.outputs().map(|o| Holder::Bar(o.name())))
            .filter_map(|h| Some((h.clone(), self.snow_edge(&h, now)?)))
            .collect();
        // The screens it snows on, and the caps over each.
        let mut on: Vec<(String, Vec<Holder>)> = Vec::new();
        let mut fields = Vec::new();
        for output in self.space.outputs() {
            let Some(area) = self.space.output_geometry(output) else {
                continue;
            };
            let name = output.name();
            let holders: Vec<Holder> = shown
                .iter()
                .filter(|(h, edge)| match h {
                    Holder::Bar(bar) => *bar == name,
                    Holder::Window(_) => edge.overlaps(area.to_f64()),
                })
                .map(|(h, _)| h.clone())
                .collect();
            let active = self.output_powered(output) && !self.wall_hidden(output);
            fields.push((name.clone(), area, holders.clone()));
            if active {
                // Locked, the windows and the bar are not shown: it falls
                // to the ground alone (#807).
                let lands = if self.locked() { Vec::new() } else { holders };
                on.push((name, lands));
            }
        }
        let falling = snowing == Snowing::Falling;
        // Each window's motion, and what it sheds.
        let mut sheds = Vec::new();
        for (holder, _) in &shown {
            let Holder::Window(id) = holder else {
                continue;
            };
            let Some(visual) = self.visual_rect(*id, now) else {
                continue;
            };
            let motion = self
                .fx
                .motions
                .track(*id, (visual.loc.x, visual.loc.y), now);
            if let Some(strength) = shed_strength(motion.acc).filter(|_| falling)
                && let Some((name, _)) = on.iter().find(|(_, hs)| hs.contains(holder))
            {
                tracing::trace!(window = id.0, acc = motion.acc, strength, "sheds its snow");
                sheds.push((holder.clone(), name.clone(), motion.v, strength));
            }
        }
        let windows = &self.windows;
        let model = &self.model;
        self.fx
            .motions
            .retain(|id| windows.contains_key(&id) && !model.is_minimized(id));
        let bars: Vec<String> = self.panel.bars.keys().cloned().collect();
        let Some(snow) = self.fx.snow.as_mut() else {
            return false;
        };
        for (holder, edge) in &shown {
            snow.place(
                holder,
                edge.loc.x as f32,
                edge.loc.y as f32,
                edge.size.w as f32,
            );
        }
        snow.retain_caps(|h| match h {
            Holder::Window(id) => windows.contains_key(id) && !model.is_minimized(*id),
            Holder::Bar(name) => bars.contains(name),
        });
        for (name, area, holders) in &fields {
            snow.field(
                name,
                (area.loc.x as f32, area.loc.y as f32),
                (area.size.w as f32, area.size.h as f32),
                holders,
            );
        }
        let names: Vec<&str> = fields.iter().map(|(n, _, _)| n.as_str()).collect();
        snow.retain_fields(&names);
        for (holder, field, v, strength) in sheds {
            snow.shed(&holder, &field, (v.0 as f32, v.1 as f32), strength);
        }
        if !falling || on.is_empty() {
            return false;
        }
        snow.advance(now, &on);
        true
    }
}
