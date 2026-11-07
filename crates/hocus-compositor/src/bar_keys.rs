//! The bar by the keyboard (docs/design/bar.md §11, #422): `Super+B`
//! (`[shortcuts] focus-bar`) puts the keyboard into a bar; which item takes
//! it, how the keys walk the items and how `Esc` steps back are the
//! shell's (`popover` in the shell).
//!
//! - The bar of the focused window's screen takes it, else the one under
//!   the pointer; a screen without a bar of its own gives the first bar.
//! - While one of a bar's surfaces holds the keyboard (an item, or its
//!   popover), `Super+B` takes it to the next screen's bar, by the
//!   screens' places, left to right, then top to bottom; from the last
//!   one, back to the window that had it.
//! - The window keeps the focus meanwhile ([`crate::model`]): when the
//!   shell's surface lets the keyboard go, the compositor gives it back
//!   to that window, as for every layer surface that held it.
//! - The session locking takes it from the bar for good: once unlocked,
//!   the window has it, not the bar.

use hocus_link::ToShell;
use smithay::desktop::layer_map_for_output;

use crate::{State, focus::KeyboardFocus};

impl State {
    /// `Super+B`: into the bar of the focused window's screen, or on to
    /// the next one, or back to the window.
    pub fn focus_bar(&mut self) {
        if !self.shell_linked() || self.locked() {
            return;
        }
        let bars = self.bars_in_order();
        if bars.is_empty() {
            return;
        }
        let to = match self.bar_holding_keyboard() {
            Some(holding) => bars
                .iter()
                .position(|b| *b == holding)
                .and_then(|i| bars.get(i + 1))
                .cloned(),
            None => {
                let screen = self.bubbles_screen().map(|o| o.name());
                screen
                    .filter(|s| bars.contains(s))
                    .or_else(|| bars.first().cloned())
            }
        };
        tracing::info!(
            bar = to.as_deref().unwrap_or("-"),
            "the keyboard and the bar"
        );
        self.tell_shell(ToShell::BarKeyboard(to));
    }

    /// The session locks: a bar holding the keyboard lets it go, so that
    /// the window has it once unlocked.
    pub(crate) fn bar_keyboard_away(&mut self) {
        if self.shell_linked() && self.bar_holding_keyboard().is_some() {
            self.tell_shell(ToShell::BarKeyboard(None));
        }
    }

    /// The screens with a bar, left to right, then top to bottom.
    fn bars_in_order(&self) -> Vec<String> {
        let mut bars: Vec<(i32, i32, String)> = self
            .space
            .outputs()
            .filter(|o| self.panel.bars.contains_key(&o.name()))
            .filter_map(|o| {
                let at = self.space.output_geometry(o)?.loc;
                Some((at.x, at.y, o.name()))
            })
            .collect();
        bars.sort();
        bars.into_iter().map(|(_, _, name)| name).collect()
    }

    /// The screen whose bar (an item of it, or its popover) holds the
    /// keyboard, if one does.
    pub(crate) fn bar_holding_keyboard(&self) -> Option<String> {
        let focus = self.seat.get_keyboard()?.current_focus()?;
        let KeyboardFocus::Wayland(surface) = focus else {
            return None;
        };
        self.space.outputs().find_map(|output| {
            let map = layer_map_for_output(output);
            map.layers()
                .any(|l| l.wl_surface() == &surface && l.namespace().starts_with("hocus-bar"))
                .then(|| output.name())
        })
    }
}
