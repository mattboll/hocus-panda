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
//!   `Super+Shift+B` (remembered in `$XDG_STATE_HOME/hocuspanda/panel.toml`,
//!   never in the configuration), else full when the screen's logical width
//!   without the full bar is at least 1600 px.
//! - **The switch:** 240 ms along the windows' spring. The compositor and
//!   the shell compute the bar's width from the same start on the same
//!   clock; the windows that touch the bar's side settle along the same
//!   motion, in the same frames as the glass behind the bar. Nothing is
//!   sent per frame. With reduced motion, or a shell drawing on the CPU
//!   (which does not animate), the form changes at once.
//! - **Which side:** `[panel] side`, the right (the default) or the left
//!   of every screen. Everything below goes with it: the edge, the space
//!   kept, the windows that touch the bar, the popovers (on its inner
//!   side), the halo and the peeking panda. A change of side made live
//!   makes each bar again on the other side; the windows it touches settle
//!   along the switch's motion (at once with reduced motion).
//! - **The edge click:** a left press in the strip between a bar and its
//!   screen's edge, or on the bar's first pixels from that edge,
//!   switches the form. The compositor takes it itself (`bar_edge_at`): it
//!   knows where the pointer is even when it entered the bar pushed against
//!   the screen's edge, with no motion after.
//! - **Under the windows:** the bar's own surfaces are drawn under normal
//!   windows; the pointer resting against the screen's edge brings a bar in
//!   front (`crate::front`, #519).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use hocus_link::{Form, Side, ToShell};
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

/// Whether screen `name` is a built-in panel.
pub(crate) fn built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|b| name.starts_with(b))
}

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
    /// The forms chosen with a click or `Super+Shift+B`, this session or
    /// remembered from the last ones, by screen name.
    chosen: BTreeMap<String, Form>,
    /// The screen the rule took when none was named and no built-in panel
    /// was there: the bar comes back to it when it comes back.
    pub(crate) home: Option<String>,
    /// Commits of the bars' surfaces, for tests: none at rest.
    pub commits: u64,
    /// Commits of the clocks' surfaces, for tests: one a second at rest.
    pub clock_commits: u64,
    /// Commits of the levels' surfaces, for tests: none at rest.
    pub levels_commits: u64,
    /// Commits of the signs of what is in use (5.12), for tests: none at
    /// rest.
    pub in_use_commits: u64,
    /// Commits of the next meeting's surfaces (Day.2), for tests: none at
    /// rest, one each minute's change.
    pub meeting_commits: u64,
    /// Commits of the meetings' reminders' tabs (Day.6), for tests: none at
    /// rest.
    pub reminder_commits: u64,
    /// Commits of Today's surfaces (Day.3), for tests: none at rest.
    pub today_commits: u64,
    /// Commits of the windows' surfaces in the bar (#604), for tests: none
    /// at rest.
    pub windows_commits: u64,
    /// Commits of the connections' surfaces, for tests: none at rest.
    pub connections_commits: u64,
    /// Commits of the desktops' surfaces, for tests: none at rest.
    pub desktops_commits: u64,
    /// Commits of the to-do list's surfaces (Day.4), for tests: none at
    /// rest.
    pub tasks_commits: u64,
    /// Commits of the popovers' surfaces, for tests: none at rest.
    pub popover_commits: u64,
    /// Commits of the peek's surfaces, for tests: none at rest.
    pub peek_commits: u64,
    /// The bar's own surfaces asked for on no screen, for tests: the
    /// shell names the screen of each (#540).
    pub asked_nowhere: u64,
    /// The shell's popovers beside the bars, and their motion.
    pub popovers: crate::popover::Popovers,
    /// The shell's launcher, and its motion (5.1).
    pub launchers: crate::launcher::Launchers,
    /// Commits of the launcher's surfaces, for tests.
    pub launcher_commits: u64,
    /// The other screens' names, as last told to the shell.
    screens: Option<Vec<String>>,
    /// Where the bars stand to the windows: under them, or one brought in
    /// front by the pointer pushed against its edge (#519).
    pub front: crate::front::Front,
}

/// A screen's model, as its EDID names it (none for the compositor's own
/// outputs, headless or nested): kept in its output's user data.
#[derive(Debug, Clone)]
pub struct ScreenModel(pub String);

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

    /// The form chosen on screen `name` with a click or `Super+Shift+B`.
    pub fn chosen(&self, name: &str) -> Option<Form> {
        self.chosen.get(name).copied()
    }

    /// The bar's width on screen `name` once its switch is over: the space
    /// kept for it.
    pub fn width(&self, name: &str) -> Option<i32> {
        self.bars.get(name).map(|b| b.form.width())
    }

    /// Switches over by `now` end; whether one, or a popover's motion
    /// (`popover_motion`), is still under way.
    pub fn tick(&mut self, now: Duration, popover_motion: bool) -> bool {
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
        moving || self.popovers.tick(now, popover_motion) || self.launchers.tick(now)
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

/// The screens among `names` (in the desktop's order) that the backlight
/// does not light: all but the built-in panel, or but the first screen
/// without one.
pub fn other_screens(names: &[String]) -> Vec<usize> {
    let lit = names
        .iter()
        .position(|n| BUILT_IN.iter().any(|b| n.starts_with(b)))
        .or((!names.is_empty()).then_some(0));
    (0..names.len()).filter(|i| Some(*i) != lit).collect()
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
                let primary = primary(&names, &self.primary_wanted(), self.panel.home.as_deref());
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
            self.bar_front_gone(&name);
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
            let scale = self.bar_scale(&name);
            self.tell_shell(ToShell::Bar {
                output: name,
                form,
                since: 0,
                scale,
            });
            changed = true;
        }
        if changed {
            self.rearrange();
            self.redraw = true;
        }
        self.tell_screens(false);
        // The rows of desktops the bars show (8.4).
        self.tell_desk_screens();
        // The primary screen may have changed: the Settings are told.
        self.displays.dirty = true;
        // Locked (7.1): the form may move to another screen; an external
        // locker's surfaces take their screens' sizes.
        self.tell_lock();
        self.external_screens_changed();
    }

    /// Screen `name`'s scale changed (6.9): its bar, if it has one, is
    /// made again at that scale by the next [`State::panel_refresh`].
    pub(crate) fn bar_rescaled(&mut self, name: &str) {
        if self.panel.bars.remove(name).is_some() {
            self.bar_front_gone(name);
            self.tell_shell(ToShell::NoBar {
                output: name.to_owned(),
            });
        }
    }

    /// Tells the shell the other screens' names (bar.md §9, the
    /// brightness's popover), when they changed or `again`.
    pub fn tell_screens(&mut self, again: bool) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let names: Vec<String> = outputs.iter().map(Output::name).collect();
        let screens: Vec<String> = other_screens(&names)
            .into_iter()
            .map(|i| {
                let output = &outputs[i];
                let model = output.user_data().get::<ScreenModel>();
                let name = model.map_or_else(|| output.name(), |m| m.0.clone());
                hocus_link::screen_name(&name)
            })
            .collect();
        if !again && self.panel.screens.as_ref() == Some(&screens) {
            return;
        }
        self.panel.screens = Some(screens.clone());
        self.tell_shell(ToShell::Screens(screens));
    }

    /// The form a bar starts in on screen `name`, `width` logical pixels
    /// wide.
    fn first_form(&self, name: &str, width: i32) -> Form {
        self.form_of(self.config.form_choice(name), name, width)
    }

    /// The form `choice` gives screen `name`, `width` logical pixels wide.
    fn form_of(&self, choice: FormChoice, name: &str, width: i32) -> Form {
        match choice {
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

    /// The configuration changed (6.4): the screens with a bar, each
    /// screen's form (a changed choice forgets the one made with a click),
    /// the clock's look and what the column shows follow it; `followed`
    /// says which. Before the screens take `[outputs]`: the forms asked
    /// until now are read from the configuration still held.
    pub(crate) fn panel_config_changed(
        &mut self,
        new: &crate::Config,
        followed: &mut Vec<&'static str>,
    ) {
        if new.panel.clock != self.config.panel.clock {
            self.config.panel.clock = new.panel.clock.clone();
            self.tell_shell(ToShell::Clock(new.panel.clock.look()));
            followed.push("[panel.clock]");
        }
        if new.panel.groups() != self.config.panel.groups() {
            let groups = new.panel.groups();
            (self.config.panel.meeting, self.config.panel.today) = (groups.meeting, groups.today);
            (self.config.panel.tasks, self.config.panel.agents) = (groups.tasks, groups.agents);
            self.config.panel.order = groups.order;
            tracing::info!(?groups, "the bar's groups");
            self.tell_shell(ToShell::BarGroups(groups));
            followed.push("[panel] groups");
        }
        // Every screen known: those with a bar, those plugged, and those
        // with a form chosen with a click.
        let mut names: Vec<String> = self.space.outputs().map(Output::name).collect();
        names.extend(self.panel.chosen.keys().cloned());
        names.sort();
        names.dedup();
        let changed: Vec<(String, FormChoice)> = names
            .into_iter()
            .filter_map(|name| {
                let now = new.form_choice(&name);
                (now != self.config.form_choice(&name)).then_some((name, now))
            })
            .collect();
        self.config.panel.form = new.panel.form;
        if !changed.is_empty() {
            let mut forgot = false;
            for (name, choice) in changed {
                forgot |= self.panel.chosen.remove(&name).is_some();
                let Some(output) = self.space.outputs().find(|o| o.name() == name).cloned() else {
                    continue;
                };
                let width = self.space.output_geometry(&output).map_or(0, |g| g.size.w);
                let form = self.form_of(choice, &name, width);
                tracing::info!(
                    output = name,
                    ?choice,
                    form = form.name(),
                    "the bar's form asked"
                );
                self.switch_bar(&name, form);
            }
            if forgot {
                self.remember_forms();
                self.displays.dirty = true;
            }
            followed.push("[panel] form");
        }
        if new.panel.screens != self.config.panel.screens {
            self.config.panel.screens = new.panel.screens;
            self.panel_refresh();
            followed.push("[panel] screens");
        }
        if new.panel.side != self.config.panel.side {
            self.move_bars(new.panel.side);
            followed.push("[panel] side");
        }
    }

    /// The side of the screens the bars stand on.
    pub fn bar_side(&self) -> Side {
        self.config.panel.side.link()
    }

    /// The sign of a shift along x toward the bar's side: what slides from
    /// the bar (a popover, a reminder's tab, the peek) slides this way.
    pub fn toward_bar(&self) -> f64 {
        -self.bar_side().inward()
    }

    /// The bars go to `side` of their screens: the shell makes each again
    /// there, and the windows that touch the space kept for it, or the
    /// space it leaves, settle along the switch's motion.
    fn move_bars(&mut self, side: crate::config::PanelSide) {
        let now = self.now();
        let reduced = self.reduced_motion() || self.shell.software;
        let outputs: Vec<Output> = self
            .space
            .outputs()
            .filter(|o| self.panel.bars.contains_key(&o.name()))
            .cloned()
            .collect();
        let before: Vec<Option<Rect>> = outputs.iter().map(|o| self.work_area(o)).collect();
        self.config.panel.side = side;
        tracing::info!(side = side.link().name(), "the bars' side");
        self.tell_shell(ToShell::BarSide(side.link()));
        let bars: Vec<(String, Form)> = self
            .panel
            .bars
            .iter_mut()
            .map(|(name, bar)| {
                // Made again on the other side: no switch carries over.
                bar.switch = None;
                (name.clone(), bar.form)
            })
            .collect();
        for (name, form) in bars {
            self.bar_front_gone(&name);
            self.tell_shell(ToShell::NoBar {
                output: name.clone(),
            });
            let scale = self.bar_scale(&name);
            self.tell_shell(ToShell::Bar {
                output: name,
                form,
                since: 0,
                scale,
            });
        }
        let motion = (!reduced).then(|| Tween::new(now, hocus_link::SWITCH, Bezier::SPRING));
        for (output, before) in outputs.iter().zip(before) {
            if let (Some(before), Some(after)) = (before, self.work_area(output)) {
                self.follow_side(output, before, after, motion);
            }
        }
        self.redraw = true;
    }

    /// The windows on `output` after the bar changed sides, its work area
    /// from `before` to `after`: snapped ones take their zone of it,
    /// floating ones in the bar's new space move out of it (narrowed only
    /// if the area is narrower), the others stay; along `motion`, else at
    /// once.
    fn follow_side(&mut self, output: &Output, before: Rect, after: Rect, motion: Option<Tween>) {
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
                Mode::Floating if rect.x < after.x || rect.right() > after.right() => {
                    let width = rect.width.min(after.width).max(model::MIN_SIZE.0);
                    let x = rect.x.clamp(after.x, (after.right() - width).max(after.x));
                    Rect::new(x, rect.y, width, rect.height)
                }
                Mode::Floating | Mode::Fullscreen => continue,
            };
            if to == rect {
                continue;
            }
            tracing::debug!(?id, ?before, ?after, ?to, "a window follows the bar's side");
            let from = self.visual_rect(id, now);
            self.configure(id, to);
            self.settle_along(id, from, motion);
        }
    }

    /// The screen whose bar's edge is under `at` (global): the strip
    /// between the bar and the screen's edge on its side, and the bar's
    /// first pixels from it, over the screen's whole height.
    pub fn bar_edge_at(&self, at: Point<f64, Logical>) -> Option<String> {
        let zone = f64::from(hocus_link::EDGE + EDGE_INSIDE);
        let side = self.bar_side();
        self.space
            .outputs()
            .filter(|o| self.panel.bars.contains_key(&o.name()))
            // The launcher covers the bar, and takes the press.
            .filter(|o| self.launcher_on(o).is_none())
            .find(|o| {
                self.space.output_geometry(o).is_some_and(|g| {
                    let g = g.to_f64();
                    g.contains(at)
                        && match side {
                            Side::Right => at.x >= g.loc.x + g.size.w - zone,
                            Side::Left => at.x < g.loc.x + zone,
                        }
                })
            })
            .map(Output::name)
    }

    /// Screen `name`'s scale in 120ths, as its surfaces' fractional scale
    /// says it.
    pub fn bar_scale(&self, name: &str) -> u32 {
        self.space
            .outputs()
            .find(|o| o.name() == name)
            .map_or(120, |o| {
                (o.current_scale().fractional_scale() * 120.0).round() as u32
            })
    }

    /// Screen `name` forgets the form chosen there with a click (the
    /// Settings' "Revenir à la forme de sa largeur", 6.4): in automatic, its
    /// bar takes the form of its width.
    pub(crate) fn bar_auto(&mut self, name: &str) {
        if self.panel.chosen.remove(name).is_none() {
            return;
        }
        tracing::info!(
            output = name,
            "the bar's form chosen with a click is forgotten"
        );
        self.remember_forms();
        self.displays.dirty = true;
        if self.config.form_choice(name) != FormChoice::Auto {
            return;
        }
        let Some(output) = self.space.outputs().find(|o| o.name() == name).cloned() else {
            return;
        };
        let width = self.space.output_geometry(&output).map_or(0, |g| g.size.w);
        self.switch_bar(name, Form::default_for(width));
    }

    /// The bar on screen `name` takes its other form (a click on its edge,
    /// `Super+Shift+B`); the windows beside it follow.
    pub fn toggle_bar(&mut self, name: &str) {
        let Some(form) = self.panel.bars.get(name).map(|b| b.form.other()) else {
            return;
        };
        if !self.switch_bar(name, form) {
            return;
        }
        self.panel.chosen.insert(name.to_owned(), form);
        if self.config.form_choice(name) == FormChoice::Auto {
            self.remember_forms();
        }
        self.displays.dirty = true;
    }

    /// The bar on screen `name` takes `form`, the windows beside it
    /// following; whether it has a bar there.
    fn switch_bar(&mut self, name: &str, form: Form) -> bool {
        let now = self.now();
        // A shell on the CPU draws the new form at once: the bar's space
        // and the windows do not move along a motion it does not show.
        let reduced = self.reduced_motion() || self.shell.software;
        let Some(output) = self.space.outputs().find(|o| o.name() == name).cloned() else {
            return false;
        };
        let before = self.work_area(&output);
        let Some(bar) = self.panel.bars.get_mut(name) else {
            return false;
        };
        if bar.form == form {
            return true;
        }
        let from = bar.width_at(now);
        bar.form = form;
        bar.switch = (!reduced).then_some((from, now));
        tracing::info!(output = name, form = form.name(), "the bar switches");
        self.displays.dirty = true;
        self.tell_shell(ToShell::Bar {
            output: name.to_owned(),
            form,
            since: if reduced { 0 } else { micros(now) },
            scale: self.bar_scale(name),
        });
        if let (Some(before), Some(after)) = (before, self.work_area(&output)) {
            let motion = (!reduced).then(|| Tween::new(now, hocus_link::SWITCH, Bezier::SPRING));
            self.follow_bar(&output, before, after, motion);
        }
        self.redraw = true;
        true
    }

    /// The windows on `output` that touch the bar's side take the new work
    /// area: snapped ones their zone of it, floating ones their edge on the
    /// bar's side at its edge; along `motion`, else at once.
    fn follow_bar(&mut self, output: &Output, before: Rect, after: Rect, motion: Option<Tween>) {
        let now = self.now();
        let side = self.bar_side();
        for id in self.windows_on(output) {
            if self.model.is_minimized(id) {
                continue;
            }
            let Some(rect) = self.target_rect(id) else {
                continue;
            };
            let to = match self.model.mode(id) {
                Mode::Snapped(zone) => zone.rect(after),
                Mode::Floating if side == Side::Left && rect.x == before.x => {
                    let width = (rect.right() - after.x).max(model::MIN_SIZE.0);
                    Rect::new(rect.right() - width, rect.y, width, rect.height)
                }
                Mode::Floating if side == Side::Right && rect.right() == before.right() => {
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
        let x = match self.bar_side() {
            Side::Right => f64::from(size.w) - edge - width,
            Side::Left => edge,
        };
        Some(Rectangle::new(
            (x, edge).into(),
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
                "# The bar's form on each screen, as last chosen with a click or Super+Shift+B.\n\
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
        // The backlight lights the built-in panel, else the first screen.
        assert_eq!(other_screens(&screens), [0, 2]);
        assert_eq!(other_screens(&names(&["DP-1", "HDMI-A-1"])), [1]);
        assert!(other_screens(&names(&["eDP-1"])).is_empty());
        assert!(other_screens(&[]).is_empty());
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
