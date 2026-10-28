//! The bar's place on the screens (docs/design/bar.md §2, §3): which
//! screens have a bar, in which form, and the space kept for it.
//!
//! The compositor decides all of it and keeps the bar's space whether the
//! shell runs or not, so that nothing moves while the shell restarts; the
//! shell draws the bars where it is told (`crate::shell`).
//!
//! - **Where:** `[panel] screens = "primary"` (the default) puts one bar on
//!   the primary screen: `[panel] output` when it is connected, else the
//!   built-in panel (eDP, LVDS, DSI), else the first screen. When that
//!   screen goes, the bar goes to one still there, and comes back with it.
//!   `"all"` puts one on every screen.
//! - **Which form:** `[outputs.NAME] panel-form`, else `[panel] form`; on
//!   `auto` (the default), the form last chosen there with a click or
//!   `Super+B` (remembered in `$XDG_STATE_HOME/hocuspanda/panel.toml`, never
//!   in the configuration), else full when the screen's logical width
//!   without the full bar is at least 1600 px.
//! - **The switch:** 240 ms along the windows' spring. The compositor and
//!   the shell compute the bar's width from the same start on the same
//!   clock; the windows that touch the bar's side settle along the same
//!   motion, in the same frames as the glass behind the bar. Nothing is
//!   sent per frame. With reduced motion, or a shell drawing on the CPU
//!   (which does not animate), the form changes at once.
//! - **The edge click:** a left press in the strip between a bar and its
//!   screen's right edge, or on the bar's first pixels from that edge,
//!   switches the form. The compositor takes it itself (`bar_edge_at`): it
//!   knows where the pointer is even when it entered the bar pushed against
//!   the screen's edge, with no motion after.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use hocus_link::{Form, ToShell};
use serde::{Deserialize, Serialize};
use smithay::{
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    State,
    config::{FormChoice, PanelScreens},
    model::{self, Mode, Rect, WindowId},
    motion::{Bezier, Tween},
};

/// The connector names of built-in panels, which are primary when no
/// screen is named.
const BUILT_IN: [&str; 3] = ["eDP", "LVDS", "DSI"];

/// Of the bar itself, the part next to the screen's edge where a press
/// switches the form, with the strip between the bar and that edge.
const EDGE_INSIDE: i32 = 6;

/// What the desktop remembers of the bar between sessions.
const STATE_FILE: &str = "panel.toml";

/// A bar on a screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub form: Form,
    /// The form switch under way: from that width, since then (the
    /// monotonic clock).
    pub switch: Option<(f64, Duration)>,
}

impl Bar {
    /// Its width as drawn at `now`, logical pixels.
    pub fn width_at(&self, now: Duration) -> f64 {
        match self.switch {
            Some((from, since)) => hocus_link::width_at(from, self.form, now.saturating_sub(since)),
            None => f64::from(self.form.width()),
        }
    }
}

/// The bars and what decides them.
#[derive(Debug, Default)]
pub struct Panel {
    /// The screens with a bar, by name.
    pub bars: BTreeMap<String, Bar>,
    /// The forms chosen with a click or `Super+B`, this session or
    /// remembered from the last ones, by screen name.
    chosen: BTreeMap<String, Form>,
    /// The screen the rule took when none was named and no built-in panel
    /// was there: the bar comes back to it when it comes back.
    home: Option<String>,
    /// Commits of the bars' surfaces, for tests: none at rest.
    pub commits: u64,
    /// Commits of the clocks' surfaces, for tests: one a second at rest.
    pub clock_commits: u64,
}

/// The file of remembered forms.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Remembered {
    #[serde(default)]
    forms: BTreeMap<String, String>,
}

impl Panel {
    /// The forms remembered in `state_dir`, read once as the compositor
    /// starts, before its loop runs.
    pub fn load(state_dir: Option<&Path>) -> Self {
        let mut panel = Self::default();
        let Some(file) = state_dir.map(|d| d.join(STATE_FILE)) else {
            return panel;
        };
        match std::fs::read_to_string(&file) {
            Ok(text) => match toml::from_str::<Remembered>(&text) {
                Ok(remembered) => {
                    panel.chosen = remembered
                        .forms
                        .into_iter()
                        .filter_map(|(name, form)| Some((name, form.parse().ok()?)))
                        .collect();
                }
                Err(err) => tracing::warn!(file = %file.display(), %err, "bar forms not read"),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(file = %file.display(), %err, "bar forms not read"),
        }
        panel
    }

    /// The bar's width on screen `name` once its switch is over: the space
    /// kept for it.
    pub fn width(&self, name: &str) -> Option<i32> {
        self.bars.get(name).map(|b| b.form.width())
    }

    /// Switches over by `now` end; whether one is still under way.
    pub fn tick(&mut self, now: Duration) -> bool {
        let mut moving = false;
        for bar in self.bars.values_mut() {
            if let Some((_, since)) = bar.switch {
                if now >= since + hocus_link::SWITCH {
                    bar.switch = None;
                } else {
                    moving = true;
                }
            }
        }
        moving
    }
}

/// The primary screen among `names` (in the desktop's order): the one
/// named, else a built-in panel, else `home` when it is there, else the
/// first.
pub fn primary(names: &[String], named: &str, home: Option<&str>) -> Option<String> {
    let there = |n: &str| names.iter().any(|m| m == n);
    if !named.is_empty() && there(named) {
        return Some(named.to_owned());
    }
    if let Some(panel) = names
        .iter()
        .find(|n| BUILT_IN.iter().any(|b| n.starts_with(b)))
    {
        return Some(panel.clone());
    }
    match home {
        Some(home) if there(home) => Some(home.to_owned()),
        _ => names.first().cloned(),
    }
}

/// `CLOCK_MONOTONIC` in microseconds, as the line to the shell carries it.
pub fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

impl State {
    /// The screens changed (or the configuration): bars go where the rule
    /// says, the shell is told, and the windows follow the space kept.
    pub fn panel_refresh(&mut self) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let names: Vec<String> = outputs.iter().map(Output::name).collect();
        let wanted: Vec<String> = match self.config.panel.screens {
            PanelScreens::All => names.clone(),
            PanelScreens::Primary => {
                let primary = primary(
                    &names,
                    &self.config.panel.output,
                    self.panel.home.as_deref(),
                );
                if self.panel.home.is_none() {
                    self.panel.home.clone_from(&primary);
                }
                primary.into_iter().collect()
            }
        };
        let gone: Vec<String> = self
            .panel
            .bars
            .keys()
            .filter(|n| !wanted.contains(n))
            .cloned()
            .collect();
        let mut changed = !gone.is_empty();
        for name in gone {
            self.panel.bars.remove(&name);
            tracing::info!(output = name, "no bar there any more");
            self.tell_shell(ToShell::NoBar { output: name });
        }
        for output in outputs {
            let name = output.name();
            if !wanted.contains(&name) || self.panel.bars.contains_key(&name) {
                continue;
            }
            let width = self.space.output_geometry(&output).map_or(0, |g| g.size.w);
            let form = self.first_form(&name, width);
            tracing::info!(output = name, form = form.name(), "a bar");
            self.panel
                .bars
                .insert(name.clone(), Bar { form, switch: None });
            self.tell_shell(ToShell::Bar {
                output: name,
                form,
                since: 0,
            });
            changed = true;
        }
        if changed {
            self.rearrange();
            self.redraw = true;
        }
    }

    /// The form a bar starts in on screen `name`, `width` logical pixels
    /// wide.
    fn first_form(&self, name: &str, width: i32) -> Form {
        match self.config.form_choice(name) {
            FormChoice::Full => Form::Full,
            FormChoice::Mini => Form::Mini,
            FormChoice::Auto => self
                .panel
                .chosen
                .get(name)
                .copied()
                .unwrap_or_else(|| Form::default_for(width)),
        }
    }

    /// The screen whose bar's edge is under `at` (global): the strip
    /// between the bar and the screen's right edge, and the bar's first
    /// pixels from it, over the screen's whole height.
    pub fn bar_edge_at(&self, at: Point<f64, Logical>) -> Option<String> {
        let zone = f64::from(hocus_link::EDGE + EDGE_INSIDE);
        self.space
            .outputs()
            .filter(|o| self.panel.bars.contains_key(&o.name()))
            .find(|o| {
                self.space.output_geometry(o).is_some_and(|g| {
                    let g = g.to_f64();
                    g.contains(at) && at.x >= g.loc.x + g.size.w - zone
                })
            })
            .map(Output::name)
    }

    /// The bar on screen `name` takes its other form (a click on its edge,
    /// `Super+B`); the windows beside it follow.
    pub fn toggle_bar(&mut self, name: &str) {
        let now = self.now();
        // A shell on the CPU draws the new form at once: the bar's space
        // and the windows do not move along a motion it does not show.
        let reduced = self.reduced_motion() || self.shell.software;
        let Some(output) = self.space.outputs().find(|o| o.name() == name).cloned() else {
            return;
        };
        let before = self.work_area(&output);
        let Some(bar) = self.panel.bars.get_mut(name) else {
            return;
        };
        let from = bar.width_at(now);
        bar.form = bar.form.other();
        bar.switch = (!reduced).then_some((from, now));
        let form = bar.form;
        tracing::info!(output = name, form = form.name(), "the bar switches");
        self.panel.chosen.insert(name.to_owned(), form);
        if self.config.form_choice(name) == FormChoice::Auto {
            self.remember_forms();
        }
        self.tell_shell(ToShell::Bar {
            output: name.to_owned(),
            form,
            since: if reduced { 0 } else { micros(now) },
        });
        if let (Some(before), Some(after)) = (before, self.work_area(&output)) {
            let motion = (!reduced).then(|| Tween::new(now, hocus_link::SWITCH, Bezier::SPRING));
            self.follow_bar(&output, before, after, motion);
        }
        self.redraw = true;
    }

    /// The windows on `output` that touch the bar's side take the new work
    /// area: snapped ones their zone of it, floating ones their right edge
    /// at its right edge; along `motion`, else at once.
    fn follow_bar(&mut self, output: &Output, before: Rect, after: Rect, motion: Option<Tween>) {
        let now = self.now();
        for id in self.windows_on(output) {
            if self.model.is_minimized(id) {
                continue;
            }
            let Some(rect) = self.target_rect(id) else {
                continue;
            };
            let to = match self.model.mode(id) {
                Mode::Snapped(zone) => zone.rect(after),
                Mode::Floating if rect.right() == before.right() => {
                    let width = (after.right() - rect.x).max(model::MIN_SIZE.0);
                    Rect::new(after.right() - width, rect.y, width, rect.height)
                }
                Mode::Floating | Mode::Fullscreen => continue,
            };
            if to == rect {
                continue;
            }
            let from = self.visual_rect(id, now);
            self.configure(id, to);
            self.settle_along(id, from, motion);
        }
    }

    /// The window `id` is drawn from `from` (global) to where it goes,
    /// along `motion`; at once without one.
    fn settle_along(
        &mut self,
        id: WindowId,
        from: Option<Rectangle<f64, Logical>>,
        motion: Option<Tween>,
    ) {
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.settle = from.zip(motion);
        }
    }

    /// The bar on `output` as drawn at `now`, output-local: where the glass
    /// goes.
    pub fn bar_rect(&self, output: &Output, now: Duration) -> Option<Rectangle<f64, Logical>> {
        let bar = self.panel.bars.get(&output.name())?;
        let size = self.space.output_geometry(output)?.size;
        let width = bar.width_at(now);
        let edge = f64::from(hocus_link::EDGE);
        Some(Rectangle::new(
            (f64::from(size.w) - edge - width, edge).into(),
            (width, f64::from(size.h) - 2.0 * edge).into(),
        ))
    }

    /// The forms chosen so far, written on a thread: a file is no work for
    /// the loop.
    fn remember_forms(&self) {
        let Some(dir) = self.config.state_dir.clone() else {
            return;
        };
        let remembered = Remembered {
            forms: self
                .panel
                .chosen
                .iter()
                .map(|(n, f)| (n.clone(), f.name().to_owned()))
                .collect(),
        };
        let text = match toml::to_string(&remembered) {
            Ok(text) => format!(
                "# The bar's form on each screen, as last chosen with a click or Super+B.\n\
                 # Written by Hocuspanda; [panel] form in config.toml wins over it.\n{text}"
            ),
            Err(err) => {
                tracing::warn!(%err, "could not write the bar's forms");
                return;
            }
        };
        crate::programs::on_a_thread("remember the bar's forms".to_owned(), move || {
            write_state(&dir, &text);
        });
    }
}

/// Writes `text` into the state file in `dir`, through a temporary file so
/// that it is never read half written.
fn write_state(dir: &PathBuf, text: &str) {
    let file = dir.join(STATE_FILE);
    let written = std::fs::create_dir_all(dir).and_then(|()| {
        let partial = dir.join(format!("{STATE_FILE}.new"));
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &file)
    });
    if let Err(err) = written {
        tracing::warn!(file = %file.display(), %err, "could not remember the bar's forms");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_primary_screen_is_the_named_one_else_the_panel_else_the_first() {
        let screens = names(&["DP-1", "eDP-1", "HDMI-A-1"]);
        assert_eq!(
            primary(&screens, "HDMI-A-1", None).as_deref(),
            Some("HDMI-A-1")
        );
        assert_eq!(primary(&screens, "DP-9", None).as_deref(), Some("eDP-1"));
        assert_eq!(primary(&screens, "", None).as_deref(), Some("eDP-1"));
        let desk = names(&["DP-1", "DP-2"]);
        assert_eq!(primary(&desk, "", None).as_deref(), Some("DP-1"));
        // The first screen it took comes back first.
        assert_eq!(primary(&desk, "", Some("DP-2")).as_deref(), Some("DP-2"));
        assert_eq!(
            primary(&names(&["DP-1"]), "", Some("DP-2")).as_deref(),
            Some("DP-1")
        );
        assert_eq!(primary(&[], "", None), None);
        assert_eq!(
            primary(&names(&["LVDS-1", "VGA-1"]), "", None).as_deref(),
            Some("LVDS-1")
        );
    }

    #[test]
    fn remembered_forms_read_back() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path().join("hocuspanda");
        write_state(
            &state,
            "[forms]\neDP-1 = \"mini\"\nDP-2 = \"full\"\nDP-3 = \"wide\"\n",
        );
        let panel = Panel::load(Some(&state));
        assert_eq!(panel.chosen.get("eDP-1"), Some(&Form::Mini));
        assert_eq!(panel.chosen.get("DP-2"), Some(&Form::Full));
        assert_eq!(panel.chosen.get("DP-3"), None);
        assert!(
            Panel::load(Some(&dir.path().join("none")))
                .chosen
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn a_switch_reversed_midway_starts_from_where_the_bar_is() {
        let ms = Duration::from_millis;
        let mut bar = Bar {
            form: Form::Mini,
            switch: Some((196.0, ms(1000))),
        };
        let midway = bar.width_at(ms(1060));
        assert!(midway < 196.0 && midway > 56.0, "{midway}");
        bar.switch = Some((midway, ms(1060)));
        bar.form = Form::Full;
        assert_eq!(bar.width_at(ms(1060)), midway);
        assert_eq!(bar.width_at(ms(1300)), 196.0);
    }
}
