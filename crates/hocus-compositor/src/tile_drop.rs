//! Dropping a window on a desktop's tile in the bar (docs/design/
//! windows.md §12, bar.md §4.4; 8.5 B, issue #490).
//!
//! While a window is moved by the pointer (its header, `Alt`+drag, the
//! grip) and the pointer comes over a tile of a bar's desktops, the bar
//! comes in front at once and stays there until the move ends; the tile
//! lights (the shell draws it, told `drop-over`); the window shrinks
//! around the grab point to 30 % of its size in 120 ms, following the
//! pointer, and the snap zones and the magnet do nothing there. Leaving
//! the tiles gives its size back in 120 ms. Let go on a tile, the window
//! goes to that desktop of that tile's screen, at the place and size it
//! had before the drag, leaving toward that desktop's side as
//! `Super+Shift+N` sends it; the screens keep the desktops they show. Let
//! go on the tile of the desktop it is on, it goes back where the drag
//! began. `Esc` during a drag puts it back there too.
//!
//! The shell lays the tiles out and says where they are on its desktops'
//! surface (`desk-tiles`); the compositor knows where that surface is.
//! Nothing runs per frame but the shrink's 120 ms.

use std::{collections::HashMap, time::Duration};

use hocus_link::{DESKTOPS_NAMESPACE, DeskTile, ToShell};
use smithay::{
    desktop::layer_map_for_output,
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    State,
    desktops::Placed,
    model::WindowId,
    motion::{self, Bezier, Tween},
};

/// The window's size over the tiles, as a share of its own.
pub const SHRUNK: f64 = 0.3;
/// How long it takes to shrink or to grow back.
pub const SHRINK: Duration = Duration::from_millis(120);

/// The tile a moved window is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Over {
    /// The bar's screen.
    pub bar: String,
    /// The tile's screen and desktop (from 0).
    pub output: String,
    pub desktop: usize,
}

/// The tiles, and a moved window over them.
#[derive(Debug, Default)]
pub struct TileDrop {
    /// Each bar's tiles on its desktops' surface, by the bar's screen.
    tiles: HashMap<String, Vec<DeskTile>>,
    /// The tile the window being moved is over.
    pub over: Option<Over>,
    /// The window's size on its way: from that share of it, toward small
    /// or back, along that tween.
    shrink: Option<(f64, bool, Tween)>,
    /// The move under way was cancelled (`Esc`): it goes back.
    pub cancelled: bool,
}

impl TileDrop {
    /// The moved window's share of its size at `now`.
    pub fn scale(&self, now: Duration) -> f64 {
        match self.shrink {
            Some((from, small, tween)) => {
                let to = if small { SHRUNK } else { 1.0 };
                if tween.done(now) {
                    return to;
                }
                motion::lerp(from, to, tween.at(now))
            }
            None => 1.0,
        }
    }

    /// Whether the window is still growing or shrinking at `now`; the
    /// shrink grown back is forgotten.
    pub fn tick(&mut self, now: Duration) -> bool {
        match self.shrink {
            Some((_, small, tween)) if tween.done(now) => {
                if !small {
                    self.shrink = None;
                }
                false
            }
            Some(_) => true,
            None => false,
        }
    }

    /// Toward small, or back, from where it is at `now`.
    fn shrink_toward(&mut self, small: bool, now: Duration, reduced: bool) {
        let from = self.scale(now);
        let duration = if reduced { Duration::ZERO } else { SHRINK };
        self.shrink = Some((from, small, Tween::new(now, duration, Bezier::EASE_OUT)));
    }
}

impl State {
    /// The shell says where bar `bar`'s tiles are.
    pub fn desk_tiles_said(&mut self, bar: String, tiles: Vec<DeskTile>) {
        self.tile_drop.tiles.insert(bar, tiles);
    }

    /// Every bar's tiles, global, of the bars still there whose desktops'
    /// surface shows.
    pub fn tile_rects(&self) -> Vec<(Over, Rectangle<f64, Logical>)> {
        self.tile_drop
            .tiles
            .iter()
            .filter(|(bar, _)| self.panel.bars.contains_key(*bar))
            .filter_map(|(bar, tiles)| {
                let output = self.space.outputs().find(|o| o.name() == *bar)?;
                Some((bar, tiles, desktops_surface_at(self, output)?))
            })
            .flat_map(|(bar, tiles, origin)| {
                tiles.iter().map(move |tile| {
                    let [x, y, width, height] = tile.rect;
                    let rect = Rectangle::<f64, Logical>::new(
                        (origin.x + f64::from(x), origin.y + f64::from(y)).into(),
                        (f64::from(width), f64::from(height)).into(),
                    );
                    let over = Over {
                        bar: bar.clone(),
                        output: tile.output.clone(),
                        desktop: usize::from(tile.desktop),
                    };
                    (over, rect)
                })
            })
            .collect()
    }

    /// The tile under `at` (global).
    pub fn tile_under(&self, at: Point<f64, Logical>) -> Option<Over> {
        self.tile_rects()
            .into_iter()
            .find(|(_, rect)| rect.contains(at))
            .map(|(over, _)| over)
    }

    /// The window being moved is over `over` now: the shell lights that
    /// tile (or none), the bar comes in front, the window shrinks or grows
    /// back. Whether it is over one.
    pub fn drag_over_tile(&mut self, over: Option<Over>) -> bool {
        let is_over = over.is_some();
        if over == self.tile_drop.over {
            return is_over;
        }
        let now = self.now();
        let reduced = self.reduced_motion();
        let before = std::mem::replace(&mut self.tile_drop.over, over.clone());
        match (&before, &over) {
            (Some(was), None) => self.tell_shell(ToShell::DropOver {
                bar: was.bar.clone(),
                over: None,
            }),
            (Some(was), Some(now)) if was.bar != now.bar => self.tell_shell(ToShell::DropOver {
                bar: was.bar.clone(),
                over: None,
            }),
            _ => {}
        }
        if let Some(over) = &over {
            self.tell_shell(ToShell::DropOver {
                bar: over.bar.clone(),
                over: Some((
                    over.output.clone(),
                    u8::try_from(over.desktop).unwrap_or(u8::MAX),
                )),
            });
            self.bar_front_dropping(&over.bar);
        }
        if before.is_some() != is_over {
            tracing::debug!(over = ?over, "a moved window over the bar's tiles");
            self.tile_drop.shrink_toward(is_over, now, reduced);
            // Small, it does not bend: the jelly would stretch the
            // shrunk window from its full size's corners.
            if is_over
                && let Some(id) = self.moving
                && self.wobble.as_ref().is_some_and(|(w, _)| *w == id)
            {
                self.wobble = None;
            }
        }
        self.redraw = true;
        is_over
    }

    /// The move of window `id`, which began as `start`, ends: let go over a
    /// tile, it goes there (see the module's documentation). Whether it
    /// was over one.
    pub fn drop_on_tile(&mut self, id: WindowId, start: Option<&Placed>) -> bool {
        let Some(over) = self.tile_drop.over.take() else {
            self.tile_drop.shrink = None;
            return false;
        };
        self.tile_drop.shrink = None;
        self.tell_shell(ToShell::DropOver {
            bar: over.bar.clone(),
            over: None,
        });
        let Some(start) = start else {
            return true;
        };
        let Some(target) = self
            .space
            .outputs()
            .find(|o| o.name() == over.output)
            .cloned()
        else {
            self.put_back(id, start);
            return true;
        };
        let desktop = over.desktop.min(self.strip(&target).count() - 1);
        tracing::debug!(
            window = id.0,
            output = over.output,
            desktop = desktop + 1,
            "a window let go on a desktop's tile"
        );
        if target == start.output {
            self.put_back(id, start);
            if desktop != self.model.desktop(id) {
                self.send_window_to_desktop(id, desktop, false);
            }
        } else {
            self.carry_to_screen(id, start, &target, desktop);
        }
        true
    }

    /// Puts window `id` back as `start` says, on its screen: its state, its
    /// place and where it floats again.
    pub(crate) fn put_back(&mut self, id: WindowId, start: &Placed) {
        if self.model.mode(id) != start.mode {
            self.model
                .set_mode(id, start.mode, start.restore.unwrap_or(start.rect));
        }
        self.model.set_restore(id, start.restore);
        // Back in sight on its screen, on the desktop that shows (the one
        // it was on, unless the screen switched during the drag).
        let shown = self.strip(&start.output).current();
        self.model.set_desktop(id, shown);
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.snapped_on = Some(start.output.clone());
        }
        self.start_settle(id);
        self.configure(id, start.rect);
    }

    /// The move under way is cancelled (`Esc`): the window goes back where
    /// the drag began.
    pub fn cancel_move(&mut self) {
        if self.moving.is_none() {
            return;
        }
        self.tile_drop.cancelled = true;
        if let Some(pointer) = self.seat.get_pointer() {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let time = self.input_time();
            pointer.unset_grab(self, serial, time);
        }
    }
}

/// Where `output`'s bar's desktops' surface is, global.
fn desktops_surface_at(state: &State, output: &Output) -> Option<Point<f64, Logical>> {
    let screen = state.space.output_geometry(output)?;
    let map = layer_map_for_output(output);
    let layer = map.layers().find(|l| l.namespace() == DESKTOPS_NAMESPACE)?;
    let geometry = map.layer_geometry(layer)?;
    Some((screen.loc + geometry.loc).to_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_shrinks_to_30_percent_in_120_ms_and_grows_back() {
        let ms = Duration::from_millis;
        let mut drop = TileDrop::default();
        assert_eq!(drop.scale(ms(0)), 1.0);
        drop.shrink_toward(true, ms(1000), false);
        assert!(drop.tick(ms(1060)));
        let half = drop.scale(ms(1060));
        assert!(half < 1.0 && half > SHRUNK, "{half}");
        assert!(!drop.tick(ms(1120)));
        assert_eq!(drop.scale(ms(1120)), SHRUNK);
        // Back, from where it is, and forgotten once grown.
        drop.shrink_toward(false, ms(2000), false);
        assert_eq!(drop.scale(ms(2000)), SHRUNK);
        assert!(!drop.tick(ms(2120)));
        assert_eq!(drop.scale(ms(2120)), 1.0);
        assert!(drop.shrink.is_none());
        // Reduced motion: at once.
        drop.shrink_toward(true, ms(3000), true);
        assert_eq!(drop.scale(ms(3000)), SHRUNK);
    }
}
