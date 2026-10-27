//! The discreet dock (#38): on the primary screen, hidden while working.
//! It shows on an empty desktop, when the pointer rests at the bottom edge
//! for 300 ms (going once the pointer leaves it), or with its shortcut;
//! hidden, a thin hint marks where it is. It holds the pinned apps, then
//! the open ones: a click focuses an open app's window, or launches it,
//! its square bouncing. An app with a window asking for attention wiggles
//! once when the dock shows, its dot in the accent, and the hint takes
//! the accent meanwhile (#145). Its last button, after a line, opens the
//! launcher on every application (#254).

use std::{collections::BTreeSet, time::Duration};

use hocus_core::WindowId;
use hocus_platform::apps::Entry;
use hocus_shell::dock::{Dock as DockView, DockApp, DockHint, Spot, look_of};
use smithay::{
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
};

use crate::{
    State,
    scene::SceneElement,
    shell::Panel,
    surface::{hash_title_and_app_id, title_and_app_id},
};

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
    /// Bumped when the entries are read again.
    pub entries_generation: u64,
    /// Whether the entries read at start came in, changed or not: until
    /// then the dock may not hold what it will (tests wait for it).
    pub entries_read: bool,
    items: Vec<Item>,
    /// What the items were built from, summed up: they are built again
    /// only when it changes, not after every turn of the loop (#88).
    inputs: Option<u64>,
    panel: Option<(String, Point<i32, Logical>, Panel<DockView>)>,
    /// The hint, and whether it says a window asks for attention.
    hint: Option<(bool, Panel<DockHint>)>,
    /// Windows asking for attention whose icon wiggled already: once.
    wiggled: BTreeSet<WindowId>,
    /// The theme it was drawn in: glass, the accent and the hour's shadow
    /// come with it; and the language its button speaks.
    theme: Option<hocus_ui::Theme>,
    /// Read again only when the configuration changes: it says it.
    lang: Option<(u64, hocus_shell::Lang)>,
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

    /// What [`State::dock_items`] reads, summed up without allocating.
    fn dock_inputs(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        // The pinned apps are the configuration's.
        self.config_generation.hash(&mut hasher);
        self.dock.entries_generation.hash(&mut hasher);
        for (id, managed) in &self.windows {
            id.hash(&mut hasher);
            hash_title_and_app_id(&managed.window, &mut hasher);
            self.asks_attention(*id).hash(&mut hasher);
        }
        hasher.finish()
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
                        attention: false,
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
            let attention = self.asks_attention(*id);
            if let Some(item) = items.iter_mut().find(|i| i.app.key == key) {
                item.windows.push(*id);
                item.app.open = true;
                item.app.attention |= attention;
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
                    attention,
                },
                exec: entry.map(|e| e.exec.clone()).unwrap_or_default(),
                windows: vec![*id],
            });
        }
        items
    }

    /// Where the dock holding `apps` apps goes on `output`, output-local:
    /// centred on the work area, beside the panel; its picture's room for
    /// the shadow puts the pill as far from the bottom edge.
    fn dock_place(&self, output: &Output, apps: usize) -> Point<i32, Logical> {
        let size = DockView::size(apps);
        let area = self.work_area(output);
        let origin = self
            .space
            .output_geometry(output)
            .map_or(area.loc, |g| g.loc);
        Point::from((
            area.loc.x - origin.x + ((area.size.w as f32 - size.w) / 2.0).round() as i32,
            area.loc.y - origin.y + area.size.h - size.h.round() as i32,
        ))
    }

    /// The shortcut: shows it, or hides it.
    pub fn toggle_dock(&mut self) {
        self.dock.toggled = !self.dock.toggled;
        self.redraw.all();
    }

    /// After each dispatch: what it holds, whether it shows, and its
    /// frames while an icon moves.
    pub fn refresh_dock(&mut self) {
        let now = self.clock.now();
        let Some(output) = self.dock_output().filter(|_| self.config.dock.enabled) else {
            self.drop_dock();
            return;
        };
        // Built again only when what they are made of changed.
        let inputs = self.dock_inputs();
        let items = (self.dock.inputs != Some(inputs)).then(|| self.dock_items());
        self.dock.inputs = Some(inputs);
        // Nothing to hold: neither it nor its hint.
        if items
            .as_ref()
            .map_or(self.dock.items.is_empty(), Vec::is_empty)
        {
            self.dock.items.clear();
            self.drop_dock();
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
        // The dock shows on its screen alone (#244).
        if self.dock.shown != shown {
            self.redraw.output(&output);
        }
        self.dock.shown = shown;
        // What it holds, built again when it changes.
        let changed = items.as_ref().is_some_and(|items| {
            items.len() != self.dock.items.len()
                || items
                    .iter()
                    .zip(&self.dock.items)
                    .any(|(a, b)| a.app != b.app)
        });
        let theme = self.theme();
        let lang = match self.dock.lang {
            Some((generation, lang)) if generation == self.config_generation => lang,
            _ => self.shell_lang(),
        };
        let scale = output.current_scale().fractional_scale() as f32;
        let restyled = self.dock.theme.as_ref() != Some(&theme);
        if restyled {
            self.dock.theme = Some(theme.clone());
            self.dock.hint = None;
        }
        let relabelled = self.dock.lang.is_none_or(|(_, l)| l != lang);
        self.dock.lang = Some((self.config_generation, lang));
        let here = self
            .dock
            .panel
            .as_ref()
            .map(|(o, _, _)| *o == output.name());
        if changed || restyled || relabelled || here != Some(true) {
            if let Some(items) = items {
                self.dock.items = items;
            }
            let apps: Vec<DockApp> = self.dock.items.iter().map(|i| i.app.clone()).collect();
            let view = DockView::new(theme.clone(), lang, scale, &apps);
            let place = self.dock_place(&output, apps.len());
            let mut panel = Panel::new(view);
            panel.upload(&[]);
            self.dock.panel = Some((output.name(), place, panel));
            // Gone from another screen too.
            if here == Some(false) {
                self.redraw.all();
            } else {
                self.redraw.output(&output);
            }
        } else {
            // The work area changes on its own: the panel takes another
            // form, or is placed after the dock was (#287).
            let place = self.dock_place(&output, self.dock.items.len());
            if let Some((_, at, _)) = &mut self.dock.panel
                && *at != place
            {
                *at = place;
                self.redraw.output(&output);
            }
        }
        let attention = self.dock.items.iter().any(|i| i.app.attention);
        if self.dock.hint.as_ref().is_none_or(|(a, _)| *a != attention) {
            let mut hint = Panel::new(DockHint::new(theme, scale, attention));
            hint.upload(&[]);
            self.dock.hint = Some((attention, hint));
            self.redraw.output(&output);
        }
        // Shown, an app with a window newly asking for attention wiggles,
        // once; hidden, it waits for the dock to show.
        let activity = &self.activity;
        let dock = &mut self.dock;
        dock.wiggled.retain(|w| activity.asks(*w));
        let mut wiggles = Vec::new();
        if dock.shown.is_some() {
            for (i, item) in dock.items.iter().enumerate() {
                let asking = item.windows.iter().filter(|w| activity.asks(**w));
                let mut new = false;
                for w in asking {
                    new |= dock.wiggled.insert(*w);
                }
                if new {
                    wiggles.push(i);
                }
            }
        }
        if let Some((_, _, panel)) = &mut self.dock.panel {
            let drawn = panel.view.draw(now);
            // After drawing, so that it starts now, not when the dock was
            // last drawn.
            for i in wiggles {
                panel.view.wiggle(i);
            }
            if panel.upload(&drawn) || panel.view.is_animating(now) {
                self.redraw.output(&output);
            }
        }
    }

    /// The dock goes: its screen redraws without it (#244).
    fn drop_dock(&mut self) {
        if let Some((name, _, _)) = self.dock.panel.take() {
            self.redraw.named(&self.space, &name);
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
            if let Some(handle) = self.waker() {
                let woken = handle.insert_source(Timer::from_duration(REST), |_, _, app| {
                    app.timed("dock rest", |app| app.state.redraw.all());
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
                self.redraw.output(&output);
            }
            return false;
        }
        let spot = panel.view.spot_at(p);
        panel.view.hover(spot);
        self.redraw.output(&output);
        match (pressed, spot) {
            (Some(true), Some(Spot::App(i))) => self.dock_pick(i),
            // Every application, in the launcher on the dock's screen.
            (Some(true), Some(Spot::Applications)) => {
                tracing::debug!("the applications, from the dock");
                panel.view.hover(None);
                self.open_launcher();
            }
            _ => {}
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
        self.redraw.all();
    }

    /// On its screen: the dock, or the hint where it hides.
    /// The dock's panel on glass and its shadow, while it shows on `output`.
    pub(crate) fn dock_panes(&self, output: &Output) -> crate::scene::Panes {
        match &self.dock.panel {
            Some((name, place, panel))
                if *name == output.name() && self.dock.shown.is_some() && !self.locked() =>
            {
                crate::scene::Panes::of(
                    crate::blur::Surface::Dock,
                    place.to_f64(),
                    &panel.view.glass(),
                    &panel.view.shadows(),
                )
            }
            _ => crate::scene::Panes::default(),
        }
    }

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
        let Some((_, hint)) = &self.dock.hint else {
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
