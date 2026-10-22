//! The discreet dock (#38): on the primary screen, hidden while working.
//! It shows on an empty desktop, when the pointer rests at the bottom edge
//! for 300 ms (going once the pointer leaves it), or with its shortcut;
//! hidden, a thin hint marks where it is. It holds the pinned apps, then
//! the open ones: a click focuses an open app's window, or launches it,
//! its square bouncing.

use std::time::Duration;

use hocus_core::WindowId;
use hocus_platform::apps::Entry;
use hocus_shell::dock::{Dock as DockView, DockApp, DockHint, look_of};
use smithay::{
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
};

use crate::{State, scene::SceneElement, shell::Panel, surface::title_and_app_id};

/// How long the pointer rests at the bottom edge before it shows.
const REST: Duration = Duration::from_millis(300);
/// How close to the bottom edge counts as on it, logical pixels.
const EDGE: f64 = 2.0;

/// Why it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// The shown desktop has no window.
    Empty,
    /// The pointer came from the bottom edge; it goes when the pointer
    /// leaves.
    Edge,
    Shortcut,
}

/// An app in the dock: how to launch it, and its windows.
#[derive(Debug, Clone)]
struct Item {
    app: DockApp,
    exec: Vec<String>,
    windows: Vec<WindowId>,
}

#[derive(Debug, Default)]
pub struct Dock {
    /// The applications known, read off the loop.
    pub entries: Vec<Entry>,
    items: Vec<Item>,
    panel: Option<(String, Point<i32, Logical>, Panel<DockView>)>,
    hint: Option<Panel<DockHint>>,
    shown: Option<Shown>,
    /// Asked by the shortcut, until asked again.
    toggled: bool,
    /// Since when the pointer rests at the bottom edge.
    edge_since: Option<Duration>,
}

impl crate::shell::Pixels for DockView {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

impl crate::shell::Pixels for DockHint {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

impl State {
    /// The primary screen, when the shell runs: the dock is part of it.
    fn dock_output(&self) -> Option<Output> {
        self.shell.as_ref()?;
        let name = self.model.primary()?.name.0.clone();
        self.find_output(&name)
    }

    /// The pinned apps found, then the open ones.
    fn dock_items(&self) -> Vec<Item> {
        let stem = |id: &str| id.trim_end_matches(".desktop").to_ascii_lowercase();
        // An app id names its entry, or ends it (`foot` for
        // `org.codeberg.dnkl.foot.desktop`).
        let entry_of = |app_id: &str| {
            let a = app_id.to_ascii_lowercase();
            self.dock.entries.iter().find(|e| {
                let s = stem(&e.id);
                s == a || s.ends_with(&format!(".{a}"))
            })
        };
        let mut items: Vec<Item> = self
            .config
            .dock
            .pinned
            .iter()
            .filter_map(|id| self.dock.entries.iter().find(|e| e.id == *id))
            .map(|e| {
                let (glyph, color) = look_of(&e.id, &e.name);
                Item {
                    app: DockApp {
                        key: e.id.clone(),
                        name: e.name.clone(),
                        glyph,
                        color,
                        open: false,
                    },
                    exec: e.exec.clone(),
                    windows: Vec::new(),
                }
            })
            .collect();
        for (id, managed) in &self.windows {
            let (title, app_id) = title_and_app_id(&managed.window);
            let app_id = app_id.unwrap_or_default();
            let key = entry_of(&app_id).map_or_else(|| app_id.clone(), |e| e.id.clone());
            if let Some(item) = items.iter_mut().find(|i| i.app.key == key) {
                item.windows.push(*id);
                item.app.open = true;
                continue;
            }
            let entry = entry_of(&app_id);
            let name = entry
                .map(|e| e.name.clone())
                .or(title)
                .unwrap_or_else(|| app_id.clone());
            let (glyph, color) = look_of(entry.map_or(&app_id, |e| &e.id), &name);
            items.push(Item {
                app: DockApp {
                    key,
                    glyph,
                    color,
                    name,
                    open: true,
                },
                exec: entry.map(|e| e.exec.clone()).unwrap_or_default(),
                windows: vec![*id],
            });
        }
        items
    }

    /// The shortcut: shows it, or hides it.
    pub fn toggle_dock(&mut self) {
        self.dock.toggled = !self.dock.toggled;
        self.needs_redraw = true;
    }

    /// After each dispatch: what it holds, whether it shows, and its
    /// frames while an icon moves.
    pub fn refresh_dock(&mut self) {
        let now = self.clock.now();
        let Some(output) = self.dock_output().filter(|_| self.config.dock.enabled) else {
            self.dock.panel = None;
            return;
        };
        let items = self.dock_items();
        // Nothing to hold: neither it nor its hint.
        if items.is_empty() {
            self.dock.items.clear();
            self.dock.panel = None;
            self.dock.shown = None;
            return;
        }
        let empty = self
            .model
            .visible_windows(&hocus_core::OutputName(output.name()))
            .is_empty();
        let resting = self.dock.edge_since.is_some_and(|t| now >= t + REST);
        // A panel taking the keys has the screen: the dock waits.
        let shown = if self.modal_open() || (self.fullscreen_on(&output) && !self.dock.toggled) {
            None
        } else if self.dock.toggled {
            Some(Shown::Shortcut)
        } else if empty {
            Some(Shown::Empty)
        } else if resting || self.dock.shown == Some(Shown::Edge) {
            Some(Shown::Edge)
        } else {
            None
        };
        self.dock.shown = shown;
        // What it holds, built again when it changes.
        let changed = items.len() != self.dock.items.len()
            || items
                .iter()
                .zip(&self.dock.items)
                .any(|(a, b)| a.app != b.app);
        let theme = self.theme();
        let scale = output.current_scale().fractional_scale() as f32;
        if changed
            || self
                .dock
                .panel
                .as_ref()
                .is_none_or(|(o, _, _)| *o != output.name())
        {
            self.dock.items = items;
            let apps: Vec<DockApp> = self.dock.items.iter().map(|i| i.app.clone()).collect();
            let view = DockView::new(theme.clone(), scale, &apps);
            let size = DockView::size(apps.len());
            let area = self.work_area(&output);
            let origin = self
                .space
                .output_geometry(&output)
                .map_or(area.loc, |g| g.loc);
            // Centred; its picture's room for the shadow puts the pill as
            // far from the bottom edge.
            let place = Point::from((
                area.loc.x - origin.x + ((area.size.w as f32 - size.w) / 2.0).round() as i32,
                area.loc.y - origin.y + area.size.h - size.h.round() as i32,
            ));
            let mut panel = Panel::new(view);
            panel.upload(&[]);
            self.dock.panel = Some((output.name(), place, panel));
            self.needs_redraw = true;
        }
        if self.dock.hint.is_none() {
            let mut hint = Panel::new(DockHint::new(theme, scale));
            hint.upload(&[]);
            self.dock.hint = Some(hint);
        }
        if let Some((_, _, panel)) = &mut self.dock.panel {
            let drawn = panel.view.draw(now);
            if panel.upload(&drawn) || panel.view.is_animating(now) {
                self.needs_redraw = true;
            }
        }
    }

    /// The pointer: resting at the bottom edge shows it; leaving it, shown
    /// by the edge, hides it; over it, an app rises; a click picks one.
    pub fn dock_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        let Some(output) = self.dock_output() else {
            return false;
        };
        let Some(area) = self.space.output_geometry(&output) else {
            return false;
        };
        let local = at - area.loc.to_f64();
        let on_screen = local.x >= 0.0 && local.x < f64::from(area.size.w);
        let at_edge = on_screen
            && local.y >= f64::from(area.size.h) - EDGE
            && local.y <= f64::from(area.size.h);
        let now = self.clock.now();
        if at_edge && self.dock.edge_since.is_none() {
            self.dock.edge_since = Some(now);
            // Woken when the rest is long enough.
            if let Some(handle) = &self.loop_handle {
                let woken = handle.insert_source(Timer::from_duration(REST), |_, _, app| {
                    app.state.needs_redraw = true;
                    TimeoutAction::Drop
                });
                if let Err(err) = woken {
                    tracing::debug!(%err.error, "the dock will show on the next event");
                }
            }
        } else if !at_edge {
            self.dock.edge_since = None;
        }
        let Some(shown) = self.dock.shown else {
            return false;
        };
        let Some((_, place, panel)) = &mut self.dock.panel else {
            return false;
        };
        let p = hocus_ui::Point::new(
            (local.x - f64::from(place.x)) as f32,
            (local.y - f64::from(place.y)) as f32,
        );
        let over = panel.view.hit(p);
        if !over {
            panel.view.hover(None);
            // Shown by the edge, it goes once the pointer left it and the
            // edge.
            if shown == Shown::Edge && !at_edge {
                self.dock.shown = None;
                self.needs_redraw = true;
            }
            return false;
        }
        let app = panel.view.app_at(p);
        panel.view.hover(app);
        self.needs_redraw = true;
        if pressed == Some(true)
            && let Some(i) = app
        {
            self.dock_pick(i);
        }
        true
    }

    /// An app clicked: its last window, focused, or the app launched.
    fn dock_pick(&mut self, i: usize) {
        let Some(item) = self.dock.items.get(i).cloned() else {
            return;
        };
        let last = item
            .windows
            .iter()
            .filter_map(|id| self.windows.get(id))
            .map(|m| m.window.clone())
            .find(|w| Some(w) == self.focused_window().as_ref())
            .or_else(|| {
                item.windows
                    .last()
                    .and_then(|id| self.windows.get(id))
                    .map(|m| m.window.clone())
            });
        match last {
            Some(window) => self.focus(Some(&window), SERIAL_COUNTER.next_serial()),
            None => {
                if let Some((program, args)) = item.exec.split_first() {
                    tracing::info!(app = item.app.key, "launching from the dock");
                    self.launch(program, args);
                    if let Some((_, _, panel)) = &mut self.dock.panel {
                        panel.view.bounce(i);
                    }
                }
            }
        }
        self.needs_redraw = true;
    }

    /// On its screen: the dock, or the hint where it hides.
    pub fn dock_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let Some((name, place, panel)) = &self.dock.panel else {
            return Vec::new();
        };
        if *name != output.name() || self.locked() {
            return Vec::new();
        }
        if self.dock.shown.is_some() {
            return panel
                .element(renderer, output, *place)
                .map(SceneElement::Frame)
                .into_iter()
                .collect();
        }
        if self.fullscreen_on(output) {
            return Vec::new();
        }
        let Some(hint) = &self.dock.hint else {
            return Vec::new();
        };
        let area = self.work_area(output);
        let origin = self
            .space
            .output_geometry(output)
            .map_or(area.loc, |g| g.loc);
        let size = DockHint::SIZE;
        let at = Point::from((
            area.loc.x - origin.x + ((area.size.w as f32 - size.w) / 2.0).round() as i32,
            area.loc.y - origin.y + area.size.h - 6 - size.h as i32,
        ));
        hint.element(renderer, output, at)
            .map(SceneElement::Frame)
            .into_iter()
            .collect()
    }

    /// Where a window's app is in the dock, global, while it shows.
    pub fn dock_icon_of(&self, id: WindowId) -> Option<Rectangle<f64, Logical>> {
        self.dock.shown?;
        let i = self
            .dock
            .items
            .iter()
            .position(|i| i.windows.contains(&id))?;
        let (name, place, panel) = self.dock.panel.as_ref()?;
        let origin = self.space.output_geometry(&self.find_output(name)?)?.loc;
        let r = panel.view.icon_rect(i)?;
        let at = (origin + *place).to_f64();
        Some(Rectangle::new(
            (at.x + f64::from(r.x), at.y + f64::from(r.y)).into(),
            (f64::from(r.w), f64::from(r.h)).into(),
        ))
    }

    /// Its apps, for the control API.
    pub fn dock_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        if self.dock.shown.is_none() {
            return;
        }
        let Some((name, place, panel)) = &self.dock.panel else {
            return;
        };
        if let Some(g) = self
            .find_output(name)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&panel.view.access(), g.loc + *place, 0, out);
        }
    }
}
