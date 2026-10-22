//! Outputs own desktops own windows (docs/decisions/0005-per-screen-desktops.md).
//!
//! The model is pure and deterministic: the compositor tells it what
//! happened (an output was plugged, a window opened, a key asked for desktop
//! 2) and reads back what to show and what has focus.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// An output, by a name that stays the same when it is plugged again (the
/// connector, or make, model and serial).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputName(pub String);

impl From<&str> for OutputName {
    fn from(name: &str) -> Self {
        Self(name.to_string())
    }
}

impl fmt::Display for OutputName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A desktop; it keeps its id across hot plug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DesktopId(pub u32);

/// A window; ids are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// A rectangle in the global logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    UnknownOutput,
    UnknownDesktop,
    UnknownWindow,
    /// The desktop is not on that output.
    NotOnOutput,
    /// The output is already plugged.
    AlreadyPlugged,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownOutput => "no such output",
            Self::UnknownDesktop => "no such desktop",
            Self::UnknownWindow => "no such window",
            Self::NotOnOutput => "the desktop is not on that output",
            Self::AlreadyPlugged => "the output is already plugged",
        })
    }
}

impl std::error::Error for ModelError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    pub id: DesktopId,
    pub name: String,
    /// The output it belongs to; it goes back there when that output is
    /// plugged again.
    pub home: OutputName,
    /// Bottom to top.
    pub windows: Vec<WindowId>,
}

/// A plugged output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: OutputName,
    pub geometry: Rect,
    /// Its own desktops first, in order, then guests from unplugged outputs.
    pub desktops: Vec<DesktopId>,
    pub shown: DesktopId,
}

/// What an output's own desktops were, kept while it is unplugged.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Home {
    desktops: Vec<DesktopId>,
    shown: DesktopId,
}

#[derive(Debug, Default, Clone)]
pub struct Model {
    /// Plugged outputs, in the order they were plugged.
    outputs: Vec<Output>,
    homes: BTreeMap<OutputName, Home>,
    desktops: BTreeMap<DesktopId, Desktop>,
    /// The desktop of each window.
    windows: BTreeMap<WindowId, DesktopId>,
    /// Least recently focused first; the focused window is last, if it is
    /// visible.
    recent: Vec<WindowId>,
    focused: Option<WindowId>,
    /// Where unplugged outputs' desktops go first; the first output when
    /// unset or unplugged.
    primary: Option<OutputName>,
    /// Windows shown on an output when it was unplugged, moved to another
    /// output's desktop: the desktop they go back to on replug.
    returning: BTreeMap<WindowId, DesktopId>,
    minimized: BTreeSet<WindowId>,
    /// Shown on every desktop of the output hosting their own.
    pinned: BTreeSet<WindowId>,
    next_desktop: u32,
    next_window: u64,
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn outputs(&self) -> &[Output] {
        &self.outputs
    }

    pub fn output(&self, name: &OutputName) -> Option<&Output> {
        self.outputs.iter().find(|o| &o.name == name)
    }

    pub fn desktop(&self, id: DesktopId) -> Option<&Desktop> {
        self.desktops.get(&id)
    }

    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }

    /// The desktop a window is on.
    pub fn desktop_of(&self, window: WindowId) -> Option<DesktopId> {
        self.windows.get(&window).copied()
    }

    /// The plugged output hosting a desktop.
    pub fn host_of(&self, desktop: DesktopId) -> Option<&Output> {
        self.outputs.iter().find(|o| o.desktops.contains(&desktop))
    }

    /// The output showing a window: the one showing its desktop, or, for
    /// a pinned window, the one hosting it.
    pub fn output_showing(&self, window: WindowId) -> Option<&Output> {
        let desktop = self.desktop_of(window)?;
        if self.pinned.contains(&window) {
            return self.host_of(desktop);
        }
        self.outputs.iter().find(|o| o.shown == desktop)
    }

    pub fn is_pinned(&self, window: WindowId) -> bool {
        self.pinned.contains(&window)
    }

    /// Pins a window on every desktop of its output, or unpins it.
    pub fn set_pinned(&mut self, window: WindowId, pinned: bool) -> Result<(), ModelError> {
        if !self.windows.contains_key(&window) {
            return Err(ModelError::UnknownWindow);
        }
        if pinned {
            self.pinned.insert(window);
        } else {
            self.pinned.remove(&window);
        }
        self.refocus();
        Ok(())
    }

    pub fn is_visible(&self, window: WindowId) -> bool {
        !self.minimized.contains(&window) && self.output_showing(window).is_some()
    }

    pub fn is_minimized(&self, window: WindowId) -> bool {
        self.minimized.contains(&window)
    }

    /// Windows shown on an output, bottom to top: its shown desktop's, then
    /// those pinned on its other desktops; minimized ones are not.
    pub fn visible_windows(&self, output: &OutputName) -> Vec<WindowId> {
        let Some(out) = self.output(output) else {
            return Vec::new();
        };
        let shown = self
            .desktops
            .get(&out.shown)
            .map(|d| d.windows.clone())
            .unwrap_or_default();
        let pinned = out
            .desktops
            .iter()
            .filter(|d| **d != out.shown)
            .filter_map(|d| self.desktops.get(d))
            .flat_map(|d| d.windows.iter().copied())
            .filter(|w| self.pinned.contains(w));
        shown
            .into_iter()
            .chain(pinned)
            .filter(|w| !self.minimized.contains(w))
            .collect()
    }

    /// Hides a window on its desktop; focus moves on. [`Model::focus`]
    /// restores it.
    pub fn minimize(&mut self, window: WindowId) -> Result<(), ModelError> {
        if !self.windows.contains_key(&window) {
            return Err(ModelError::UnknownWindow);
        }
        self.minimized.insert(window);
        if self.focused == Some(window) {
            self.focused = None;
        }
        self.refocus();
        Ok(())
    }

    /// Plugs an output. One seen before gets its own desktops back, with
    /// their windows; a new one gets `desktops` desktops (at least one).
    pub fn plug(
        &mut self,
        name: OutputName,
        geometry: Rect,
        desktops: usize,
    ) -> Result<(), ModelError> {
        if self.output(&name).is_some() {
            return Err(ModelError::AlreadyPlugged);
        }
        let home = match self.homes.get(&name) {
            Some(home) => home.clone(),
            None => {
                let ids: Vec<DesktopId> = (1..=desktops.max(1))
                    .map(|n| self.new_desktop(n.to_string(), &name))
                    .collect();
                let home = Home {
                    shown: ids[0],
                    desktops: ids,
                };
                self.homes.insert(name.clone(), home.clone());
                home
            }
        };
        // Its desktops leave the outputs that hosted them meanwhile.
        for output in &mut self.outputs {
            output.desktops.retain(|d| !home.desktops.contains(d));
        }
        self.fix_shown();
        let mut hosted = home.desktops.clone();
        // With no output left, desktops of other unplugged outputs waited
        // for any output to come.
        if self.outputs.is_empty() {
            hosted.extend(
                self.desktops
                    .values()
                    .filter(|d| d.home != name)
                    .map(|d| d.id),
            );
        }
        self.outputs.push(Output {
            name,
            geometry,
            desktops: hosted,
            shown: home.shown,
        });
        // Windows it showed when it was unplugged come back, in their order.
        let coming_back: Vec<(WindowId, DesktopId)> = self
            .returning
            .iter()
            .filter(|(_, origin)| home.desktops.contains(origin))
            .map(|(w, d)| (*w, *d))
            .collect();
        for (window, origin) in self.in_stacking_order(coming_back) {
            self.returning.remove(&window);
            self.move_window(window, origin);
        }
        self.refocus();
        Ok(())
    }

    /// The output unplugged outputs' desktops and windows go to.
    pub fn primary(&self) -> Option<&Output> {
        self.primary
            .as_ref()
            .and_then(|name| self.output(name))
            .or_else(|| self.outputs.first())
    }

    pub fn set_primary(&mut self, name: &OutputName) -> Result<(), ModelError> {
        self.output(name).ok_or(ModelError::UnknownOutput)?;
        self.primary = Some(name.clone());
        Ok(())
    }

    /// Windows that left an unplugged output and wait to go back to it.
    pub fn is_returning(&self, window: WindowId) -> bool {
        self.returning.contains_key(&window)
    }

    /// Unplugs an output. The windows it showed move to the shown desktop of
    /// the primary output, so that nothing in sight disappears; every desktop
    /// it hosted joins that output. All go back when it is plugged again.
    pub fn unplug(&mut self, name: &OutputName) -> Result<(), ModelError> {
        let index = self
            .outputs
            .iter()
            .position(|o| &o.name == name)
            .ok_or(ModelError::UnknownOutput)?;
        let output = self.outputs.remove(index);
        if let Some(home) = self.homes.get_mut(name) {
            // The shown desktop is remembered only if it is its own.
            if home.desktops.contains(&output.shown) {
                home.shown = output.shown;
            }
        }
        let Some(host) = self.primary().map(|o| o.name.clone()) else {
            // No output left: everything waits for the next one.
            self.refocus();
            return Ok(());
        };
        let shown_windows = self
            .desktops
            .get(&output.shown)
            .map(|d| d.windows.clone())
            .unwrap_or_default();
        if let Some(host) = self.outputs.iter_mut().find(|o| o.name == host) {
            host.desktops.extend(output.desktops);
            let target = host.shown;
            for window in shown_windows {
                // A window that already moved keeps its first home.
                self.returning.entry(window).or_insert(output.shown);
                self.move_window(window, target);
            }
        }
        self.refocus();
        Ok(())
    }

    pub fn set_geometry(&mut self, name: &OutputName, geometry: Rect) -> Result<(), ModelError> {
        let output = self
            .outputs
            .iter_mut()
            .find(|o| &o.name == name)
            .ok_or(ModelError::UnknownOutput)?;
        output.geometry = geometry;
        Ok(())
    }

    /// Shows a desktop of an output; the other outputs do not change. Focus
    /// goes to that desktop: its most recently focused window, or none when
    /// it is empty, so that typing never lands on another output by surprise.
    pub fn show(&mut self, output: &OutputName, desktop: DesktopId) -> Result<(), ModelError> {
        let out = self
            .outputs
            .iter_mut()
            .find(|o| &o.name == output)
            .ok_or(ModelError::UnknownOutput)?;
        if !out.desktops.contains(&desktop) {
            return Err(ModelError::NotOnOutput);
        }
        if out.shown == desktop {
            return Ok(());
        }
        out.shown = desktop;
        self.focused = self
            .recent
            .iter()
            .rev()
            .copied()
            .find(|w| self.windows.get(w) == Some(&desktop));
        if self.focused.is_none() {
            // Windows never focused yet: the top one.
            self.focused = self
                .desktops
                .get(&desktop)
                .and_then(|d| d.windows.last().copied());
        }
        Ok(())
    }

    /// Shows an output's desktop by position, from 0.
    pub fn show_nth(&mut self, output: &OutputName, n: usize) -> Result<(), ModelError> {
        let desktop = self
            .output(output)
            .ok_or(ModelError::UnknownOutput)?
            .desktops
            .get(n)
            .copied()
            .ok_or(ModelError::UnknownDesktop)?;
        self.show(output, desktop)
    }

    /// Desktops are named "1", "2"… until named otherwise.
    pub fn rename(
        &mut self,
        desktop: DesktopId,
        name: impl Into<String>,
    ) -> Result<(), ModelError> {
        let desktop = self
            .desktops
            .get_mut(&desktop)
            .ok_or(ModelError::UnknownDesktop)?;
        desktop.name = name.into();
        Ok(())
    }

    /// Opens a window on the shown desktop of `output`, on top, focused.
    pub fn open_window(&mut self, output: &OutputName) -> Result<WindowId, ModelError> {
        let desktop = self.output(output).ok_or(ModelError::UnknownOutput)?.shown;
        self.next_window += 1;
        let id = WindowId(self.next_window);
        self.windows.insert(id, desktop);
        if let Some(d) = self.desktops.get_mut(&desktop) {
            d.windows.push(id);
        }
        self.focus(id)?;
        Ok(id)
    }

    /// Forgets a window; focus goes to the most recent visible one.
    pub fn close_window(&mut self, window: WindowId) -> Result<(), ModelError> {
        let desktop = self
            .windows
            .remove(&window)
            .ok_or(ModelError::UnknownWindow)?;
        self.returning.remove(&window);
        self.minimized.remove(&window);
        self.pinned.remove(&window);
        if let Some(d) = self.desktops.get_mut(&desktop) {
            d.windows.retain(|w| *w != window);
        }
        self.recent.retain(|w| *w != window);
        if self.focused == Some(window) {
            self.focused = None;
        }
        self.refocus();
        Ok(())
    }

    /// Leaves no window focused, as a click on the background does. The next
    /// change that needs a focused window picks the most recent visible one.
    pub fn clear_focus(&mut self) {
        self.focused = None;
    }

    /// Focuses and raises a window. A hidden window's desktop is shown first.
    pub fn focus(&mut self, window: WindowId) -> Result<(), ModelError> {
        let desktop = self.desktop_of(window).ok_or(ModelError::UnknownWindow)?;
        self.minimized.remove(&window);
        if !self.is_visible(window)
            && let Some(host) = self.host_of(desktop).map(|o| o.name.clone())
        {
            self.show(&host, desktop)?;
        }
        if let Some(d) = self.desktops.get_mut(&desktop) {
            d.windows.retain(|w| *w != window);
            d.windows.push(window);
        }
        self.recent.retain(|w| *w != window);
        self.recent.push(window);
        self.focused = self.is_visible(window).then_some(window);
        Ok(())
    }

    /// Sends a window to another output: to `desktop` if given (one of that
    /// output's), otherwise to the desktop it shows. A focused window keeps
    /// focus.
    pub fn send_to_output(
        &mut self,
        window: WindowId,
        output: &OutputName,
        desktop: Option<DesktopId>,
    ) -> Result<(), ModelError> {
        let target = self.output(output).ok_or(ModelError::UnknownOutput)?;
        let desktop = match desktop {
            Some(d) if target.desktops.contains(&d) => d,
            Some(_) => return Err(ModelError::NotOnOutput),
            None => target.shown,
        };
        self.send_to_desktop(window, desktop)
    }

    /// Moves a window to a desktop, on top. A focused window keeps focus
    /// while it stays visible. A window placed by hand stays where it is put,
    /// even when the output it left is plugged again.
    pub fn send_to_desktop(
        &mut self,
        window: WindowId,
        desktop: DesktopId,
    ) -> Result<(), ModelError> {
        if !self.desktops.contains_key(&desktop) {
            return Err(ModelError::UnknownDesktop);
        }
        if !self.windows.contains_key(&window) {
            return Err(ModelError::UnknownWindow);
        }
        self.returning.remove(&window);
        self.move_window(window, desktop);
        if self.focused == Some(window) && !self.is_visible(window) {
            self.focused = None;
        }
        self.refocus();
        Ok(())
    }

    /// Moves a known window onto a known desktop, on top.
    fn move_window(&mut self, window: WindowId, desktop: DesktopId) {
        let Some(old) = self.windows.insert(window, desktop) else {
            return;
        };
        if let Some(d) = self.desktops.get_mut(&old) {
            d.windows.retain(|w| *w != window);
        }
        if let Some(d) = self.desktops.get_mut(&desktop) {
            d.windows.push(window);
        }
    }

    /// `windows` sorted bottom to top as they stack on their desktops.
    fn in_stacking_order<T: Copy>(&self, mut windows: Vec<(WindowId, T)>) -> Vec<(WindowId, T)> {
        let rank = |w: WindowId| {
            self.windows
                .get(&w)
                .and_then(|d| self.desktops.get(d))
                .and_then(|d| d.windows.iter().position(|x| *x == w))
                .unwrap_or_default()
        };
        windows.sort_by_key(|(w, _)| rank(*w));
        windows
    }

    fn new_desktop(&mut self, name: String, home: &OutputName) -> DesktopId {
        self.next_desktop += 1;
        let id = DesktopId(self.next_desktop);
        self.desktops.insert(
            id,
            Desktop {
                id,
                name,
                home: home.clone(),
                windows: Vec::new(),
            },
        );
        id
    }

    /// An output whose shown desktop left it shows its first one.
    fn fix_shown(&mut self) {
        for output in &mut self.outputs {
            if !output.desktops.contains(&output.shown)
                && let Some(first) = output.desktops.first()
            {
                output.shown = *first;
            }
        }
    }

    /// Keeps focus on a visible window: the focused one if it still is,
    /// otherwise the most recently focused visible one.
    fn refocus(&mut self) {
        if self.focused.is_some_and(|w| self.is_visible(w)) {
            return;
        }
        self.focused = self
            .recent
            .iter()
            .rev()
            .copied()
            .find(|w| self.is_visible(*w));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laptop() -> OutputName {
        "laptop".into()
    }

    fn external() -> OutputName {
        "external".into()
    }

    const LAPTOP: Rect = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 800,
    };
    const EXTERNAL: Rect = Rect {
        x: 1280,
        y: 0,
        width: 3440,
        height: 1440,
    };

    /// A laptop with 1 desktop and an external output with 4.
    fn two_outputs() -> Model {
        let mut model = Model::new();
        model.plug(laptop(), LAPTOP, 1).expect("plug laptop");
        model.plug(external(), EXTERNAL, 4).expect("plug external");
        model
    }

    fn shown(model: &Model, output: &OutputName) -> DesktopId {
        model.output(output).map(|o| o.shown).expect("plugged")
    }

    fn desktops(model: &Model, output: &OutputName) -> Vec<DesktopId> {
        model
            .output(output)
            .map(|o| o.desktops.clone())
            .expect("plugged")
    }

    #[test]
    fn each_output_switches_alone() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let laptop_desktop = shown(&model, &laptop());

        model.show_nth(&external(), 1)?;

        assert_eq!(shown(&model, &external()), desktops(&model, &external())[1]);
        assert_eq!(shown(&model, &laptop()), laptop_desktop);
        Ok(())
    }

    #[test]
    fn a_window_sent_to_another_output_lands_on_its_shown_desktop_and_keeps_focus()
    -> Result<(), ModelError> {
        let mut model = two_outputs();
        let window = model.open_window(&external())?;

        model.send_to_output(window, &laptop(), None)?;

        assert_eq!(model.desktop_of(window), Some(shown(&model, &laptop())));
        assert_eq!(model.visible_windows(&laptop()), [window]);
        assert_eq!(model.visible_windows(&external()), []);
        assert_eq!(model.focused(), Some(window));
        Ok(())
    }

    #[test]
    fn a_window_can_be_sent_to_a_chosen_desktop_of_another_output() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let window = model.open_window(&laptop())?;
        let third = desktops(&model, &external())[2];

        model.send_to_output(window, &external(), Some(third))?;

        assert_eq!(model.desktop_of(window), Some(third));
        // The external output still shows its first desktop.
        assert!(!model.is_visible(window));
        assert_eq!(model.focused(), None);
        Ok(())
    }

    #[test]
    fn unplugged_desktops_return_with_their_windows() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let external_desktops = desktops(&model, &external());
        model.show_nth(&external(), 2)?;
        let window = model.open_window(&external())?;

        model.unplug(&external())?;

        // They joined the laptop, still knowing where they come from.
        let hosted = desktops(&model, &laptop());
        assert_eq!(hosted.len(), 5);
        for id in &external_desktops {
            assert!(hosted.contains(id));
            assert_eq!(model.desktop(*id).map(|d| d.home.clone()), Some(external()));
        }
        // The laptop still shows its own desktop.
        assert_eq!(shown(&model, &laptop()), hosted[0]);

        model.plug(external(), EXTERNAL, 4)?;

        assert_eq!(desktops(&model, &external()), external_desktops);
        assert_eq!(desktops(&model, &laptop()).len(), 1);
        assert_eq!(shown(&model, &external()), external_desktops[2]);
        assert_eq!(model.visible_windows(&external()), [window]);
        Ok(())
    }

    #[test]
    fn windows_in_sight_stay_in_sight_when_their_output_goes() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let chat = model.open_window(&laptop())?;
        let a = model.open_window(&external())?;
        let b = model.open_window(&external())?;
        model.show_nth(&external(), 1)?;
        let hidden = model.open_window(&external())?;
        model.show_nth(&external(), 0)?;
        model.focus(a)?;

        model.unplug(&external())?;

        // On the laptop's shown desktop, above its own window, in their order.
        assert_eq!(model.visible_windows(&laptop()), [chat, b, a]);
        assert_eq!(model.focused(), Some(a));
        assert!(!model.is_visible(hidden));
        assert!(model.is_returning(a));

        model.plug(external(), EXTERNAL, 4)?;

        assert_eq!(model.visible_windows(&laptop()), [chat]);
        assert_eq!(model.visible_windows(&external()), [b, a]);
        assert!(!model.is_returning(a));
        model.show_nth(&external(), 1)?;
        assert_eq!(model.visible_windows(&external()), [hidden]);
        Ok(())
    }

    #[test]
    fn a_window_moved_by_hand_meanwhile_stays_where_it_was_put() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let window = model.open_window(&external())?;
        model.unplug(&external())?;
        let laptop_desktop = shown(&model, &laptop());
        model.send_to_desktop(window, laptop_desktop)?;

        model.plug(external(), EXTERNAL, 4)?;

        assert_eq!(model.visible_windows(&laptop()), [window]);
        Ok(())
    }

    #[test]
    fn the_primary_output_takes_what_an_unplugged_one_had() -> Result<(), ModelError> {
        let mut model = two_outputs();
        model.plug("projector".into(), LAPTOP, 1)?;
        model.set_primary(&external())?;
        let shown_on_projector = model.open_window(&"projector".into())?;

        model.unplug(&"projector".into())?;

        assert_eq!(model.visible_windows(&external()), [shown_on_projector]);
        assert_eq!(model.primary().map(|o| o.name.clone()), Some(external()));
        assert_eq!(
            model.set_primary(&"projector".into()),
            Err(ModelError::UnknownOutput)
        );
        Ok(())
    }

    #[test]
    fn a_guest_desktop_shown_on_the_host_goes_home_on_replug() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let guest = desktops(&model, &external())[3];
        model.unplug(&external())?;
        model.show(&laptop(), guest)?;

        model.plug(external(), EXTERNAL, 4)?;

        assert_eq!(shown(&model, &laptop()), desktops(&model, &laptop())[0]);
        assert!(desktops(&model, &external()).contains(&guest));
        Ok(())
    }

    #[test]
    fn with_every_output_gone_desktops_wait_for_the_next_one() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let window = model.open_window(&laptop())?;
        model.unplug(&external())?;
        model.unplug(&laptop())?;
        assert_eq!(model.focused(), None);

        model.plug("projector".into(), LAPTOP, 1)?;

        assert_eq!(desktops(&model, &"projector".into()).len(), 6);
        model.unplug(&"projector".into())?;
        model.plug(laptop(), LAPTOP, 1)?;
        assert_eq!(model.visible_windows(&laptop()), [window]);
        Ok(())
    }

    #[test]
    fn closing_the_focused_window_focuses_the_previous_one() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let a = model.open_window(&laptop())?;
        let b = model.open_window(&external())?;
        let c = model.open_window(&laptop())?;
        assert_eq!(model.focused(), Some(c));

        model.close_window(c)?;

        assert_eq!(model.focused(), Some(b));
        model.close_window(b)?;
        assert_eq!(model.focused(), Some(a));
        Ok(())
    }

    #[test]
    fn focus_skips_windows_on_hidden_desktops() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let a = model.open_window(&external())?;
        model.show_nth(&external(), 1)?;
        let b = model.open_window(&external())?;
        let c = model.open_window(&laptop())?;

        model.close_window(c)?;

        assert_eq!(model.focused(), Some(b));
        // Switching back shows a; focus follows the shown windows.
        model.show_nth(&external(), 0)?;
        assert_eq!(model.focused(), Some(a));
        Ok(())
    }

    #[test]
    fn switching_a_desktop_moves_focus_there() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let chat = model.open_window(&laptop())?;
        let editor = model.open_window(&external())?;

        model.show_nth(&external(), 1)?;
        // An empty desktop: no window gets the keys meant for it.
        assert_eq!(model.focused(), None);

        model.show_nth(&external(), 0)?;
        assert_eq!(model.focused(), Some(editor));

        model.focus(chat)?;
        model.show_nth(&external(), 0)?;
        assert_eq!(
            model.focused(),
            Some(chat),
            "showing the shown desktop changes nothing"
        );
        Ok(())
    }

    #[test]
    fn focusing_raises_and_shows_the_window() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let a = model.open_window(&external())?;
        let b = model.open_window(&external())?;
        model.show_nth(&external(), 3)?;

        model.focus(a)?;

        assert_eq!(model.visible_windows(&external()), [b, a]);
        assert_eq!(model.focused(), Some(a));
        Ok(())
    }

    #[test]
    fn desktops_are_numbered_until_named() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let ids = desktops(&model, &external());
        let names: Vec<_> = ids
            .iter()
            .filter_map(|id| model.desktop(*id).map(|d| d.name.clone()))
            .collect();
        assert_eq!(names, ["1", "2", "3", "4"]);

        model.rename(ids[1], "Project")?;

        assert_eq!(
            model.desktop(ids[1]).map(|d| d.name.as_str()),
            Some("Project")
        );
        Ok(())
    }

    #[test]
    fn a_minimized_window_is_hidden_and_restored_by_focus() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let a = model.open_window(&laptop())?;
        let b = model.open_window(&laptop())?;

        model.minimize(b)?;

        assert_eq!(model.visible_windows(&laptop()), [a]);
        assert_eq!(model.focused(), Some(a));
        assert!(model.is_minimized(b));

        model.focus(b)?;

        assert_eq!(model.visible_windows(&laptop()), [a, b]);
        assert_eq!(model.focused(), Some(b));
        Ok(())
    }

    #[test]
    fn a_pinned_window_stays_when_its_output_switches() -> Result<(), ModelError> {
        let mut model = two_outputs();
        let music = model.open_window(&external())?;
        model.set_pinned(music, true)?;

        model.show_nth(&external(), 2)?;

        assert_eq!(model.visible_windows(&external()), [music]);
        assert!(model.is_visible(music));
        assert_eq!(model.visible_windows(&laptop()), []);
        Ok(())
    }

    #[test]
    fn mistakes_are_errors_not_panics() {
        let mut model = two_outputs();
        let laptop_only = desktops(&model, &laptop())[0];
        assert_eq!(
            model.show(&external(), laptop_only),
            Err(ModelError::NotOnOutput)
        );
        assert_eq!(
            model.show_nth(&laptop(), 3),
            Err(ModelError::UnknownDesktop)
        );
        assert_eq!(
            model.close_window(WindowId(99)),
            Err(ModelError::UnknownWindow)
        );
        assert_eq!(model.unplug(&"nope".into()), Err(ModelError::UnknownOutput));
        assert_eq!(
            model.plug(laptop(), LAPTOP, 1),
            Err(ModelError::AlreadyPlugged)
        );
    }
}
