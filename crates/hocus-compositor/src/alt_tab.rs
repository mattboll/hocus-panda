//! The window switcher (#39): Alt+Tab lists the windows of the screen
//! under the pointer, Super+Tab every screen's, each screen showing its
//! own; Tab, Shift+Tab and the arrows move the choice, which glides; letting
//! the modifier go, Enter or a click picks it, Escape gives up. A pinch with
//! three fingers or more opens it for every screen. Each card shows its
//! window live: the window's own surfaces drawn small in it. In the Matrix
//! theme the switcher and the overview come and go in rain
//! (`matrix_screens`).

use hocus_core::{WindowId, shortcuts::Scope};
use std::time::Duration;

use hocus_shell::{
    matrix::Variant,
    window_switcher::{Card, Desks, WindowSwitcher},
};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, surface::WaylandSurfaceRenderElement, utils::RescaleRenderElement,
        },
    },
    desktop::Window,
    input::keyboard::{Keysym, ModifiersState},
    output::Output,
    utils::{Logical, Point, Rectangle, Scale, Serial},
};

use crate::{
    State, decoration::Rounded, desktops::model_id, scene::SceneElement, shell::Panel,
    surface::title_and_app_id,
};

/// What, held, keeps the switcher open; let go, it picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Alt,
    Super,
}

/// What it was opened as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    /// Alt+Tab, Super+Tab: held while choosing.
    Switcher,
    /// Every screen's windows until one is picked: Super alone, a corner,
    /// a pinch.
    Overview,
}

/// One screen's grid.
#[derive(Debug)]
pub(crate) struct View {
    pub(crate) output: String,
    /// Its corner, logical, from the output's.
    pub(crate) place: Point<i32, Logical>,
    pub(crate) panel: Panel<WindowSwitcher>,
    /// Its cards' places in the list.
    pub(crate) first: usize,
    pub(crate) count: usize,
}

#[derive(Debug)]
pub(crate) struct Open {
    /// Every window listed, in order, with its screen.
    pub(crate) entries: Vec<(WindowId, String)>,
    pub(crate) selected: usize,
    held: Option<Held>,
    pub(crate) views: Vec<View>,
    pub(crate) shown: Shown,
    pub(crate) since: Duration,
    /// The choice moved from this card then.
    pub(crate) moved: Option<(usize, Duration)>,
}

/// Closed in the Matrix theme: it goes in its own way a moment.
#[derive(Debug)]
pub(crate) struct Closing {
    pub(crate) open: Open,
    pub(crate) since: Duration,
    /// The window picked, if one was.
    pub(crate) picked: Option<usize>,
}

#[derive(Debug, Default)]
pub struct AltTab {
    pub(crate) open: Option<Open>,
    pub(crate) closing: Option<Closing>,
    /// A pinch's fingers and how far it closed (1 where it began).
    pinch: Option<(u32, f64)>,
}

impl crate::shell::Pixels for WindowSwitcher {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

impl State {
    pub fn alt_tab_open(&self) -> bool {
        self.alt_tab.open.is_some()
    }

    /// The windows of `output`'s shown desktop, the one on top first,
    /// those minimized last.
    fn windows_of(&self, output: &Output) -> Vec<WindowId> {
        let name = hocus_core::OutputName(output.name());
        let visible = self.model.visible_windows(&name);
        let mut ids: Vec<WindowId> = self
            .space
            .elements()
            .rev()
            .filter_map(model_id)
            .filter(|id| visible.contains(id))
            .collect();
        if let Some(desk) = self
            .model
            .output(&name)
            .and_then(|o| self.model.desktop(o.shown))
        {
            ids.extend(
                desk.windows
                    .iter()
                    .rev()
                    .filter(|id| self.model.is_minimized(**id)),
            );
        }
        ids
    }

    /// The line of `output`'s desktops over its cards (#192): when one of
    /// them has a name, or something goes on on one.
    fn desks_line(&self, output: &Output) -> Option<Desks> {
        let name = hocus_core::OutputName(output.name());
        let out = self.model.output(&name)?;
        let names = self.desktop_names(&name);
        let badges = self.desktop_badges(&name);
        if names.iter().all(String::is_empty) && badges.iter().all(|b| !b.is_some()) {
            return None;
        }
        let lang = self.shell_lang();
        let names: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, n)| lang.desktop_label(n, i + 1))
            .collect();
        let said = names
            .iter()
            .zip(&badges)
            .map(|(n, b)| match lang.badge(*b) {
                Some(what) => format!("{n} · {what}"),
                None => n.clone(),
            })
            .collect();
        Some(Desks {
            names,
            said,
            badges,
            shown: out.desktops.iter().position(|d| *d == out.shown)?,
        })
    }

    /// The switcher's action: opens it, or picks the next window when it
    /// is open.
    pub fn switch_windows(&mut self, scope: Scope) {
        if self.alt_tab.open.is_some() {
            self.alt_tab_step(1);
            return;
        }
        let modifiers = self.seat.get_keyboard().map(|k| k.modifier_state());
        let held = match modifiers {
            Some(m) if m.logo => Some(Held::Super),
            Some(m) if m.alt => Some(Held::Alt),
            _ => None,
        };
        self.open_alt_tab(scope, held, Shown::Switcher);
    }

    fn open_alt_tab(&mut self, scope: Scope, held: Option<Held>, shown: Shown) {
        let pointer = self.output_under_pointer();
        let mut outputs: Vec<Output> = match scope {
            Scope::PointerOutput => pointer.iter().cloned().collect(),
            Scope::AllOutputs => self.space.outputs().cloned().collect(),
        };
        // The screen under the pointer first.
        outputs.sort_by_key(|o| Some(o) != pointer.as_ref());
        let mut entries = Vec::new();
        let mut spans = Vec::new();
        for output in &outputs {
            let first = entries.len();
            entries.extend(
                self.windows_of(output)
                    .into_iter()
                    .map(|id| (id, output.name())),
            );
            spans.push((output.clone(), first, entries.len() - first));
        }
        if entries.is_empty() {
            return;
        }
        // The window in front is the one already there: the next one first.
        let focused = self.model.focused();
        let selected = usize::from(entries.len() > 1 && entries.first().map(|e| e.0) == focused);
        let theme = self.theme();
        let variant = if self.matrix_look() {
            Variant::Matrix
        } else {
            Variant::Plain
        };
        let now = self.clock.now();
        let typed = self.matrix_moves();
        let mut views = Vec::new();
        for (output, first, count) in spans {
            if count == 0 {
                continue;
            }
            let Some(area) = self.space.output_geometry(&output) else {
                continue;
            };
            let cards: Vec<Card> = entries[first..first + count]
                .iter()
                .filter_map(|(id, _)| {
                    let window = &self.windows.get(id)?.window;
                    let (title, app_id) = title_and_app_id(window);
                    let app = app_id.unwrap_or_default();
                    Some(Card {
                        title: title
                            .filter(|t| !t.is_empty())
                            .unwrap_or_else(|| app.clone()),
                        color: hocus_shell::toasts::app_color(&app),
                    })
                })
                .collect();
            let width = (area.size.w as f32 * 0.9).max(320.0);
            let mine = (first..first + count)
                .contains(&selected)
                .then(|| selected - first);
            let desks = self.desks_line(&output);
            let mut switcher = WindowSwitcher::new(
                theme.clone(),
                output.current_scale().fractional_scale() as f32,
                &cards,
                mine,
                width,
                variant,
                desks.as_ref(),
            );
            // The Matrix theme types the titles in.
            if typed {
                switcher.type_titles(now);
            }
            let size = WindowSwitcher::size(cards.len(), width, desks.is_some());
            let place = Point::from((
                ((area.size.w as f32 - size.w) / 2.0).round() as i32,
                ((area.size.h as f32 - size.h) / 2.0).round() as i32,
            ));
            let mut panel = Panel::new(switcher);
            panel.upload(&[]);
            views.push(View {
                output: output.name(),
                place,
                panel,
                first,
                count,
            });
        }
        self.alt_tab.closing = None;
        self.alt_tab.open = Some(Open {
            entries,
            selected,
            held,
            views,
            shown,
            since: now,
            moved: None,
        });
        self.needs_redraw = true;
    }

    /// It closes, `picked` the card chosen: in the Matrix theme it goes in
    /// its own way.
    fn alt_tab_close(&mut self, picked: Option<usize>) -> Option<WindowId> {
        let open = self.alt_tab.open.take()?;
        self.needs_redraw = true;
        let id = picked.and_then(|i| open.entries.get(i)).map(|(id, _)| *id);
        if self.matrix_moves() {
            self.alt_tab.closing = Some(Closing {
                open,
                since: self.clock.now(),
                picked,
            });
        }
        id
    }

    /// Moves the choice by `step` through the list, round.
    fn alt_tab_step(&mut self, step: isize) {
        let Some(open) = &self.alt_tab.open else {
            return;
        };
        let n = open.entries.len() as isize;
        let next = (open.selected as isize + step).rem_euclid(n) as usize;
        self.alt_tab_select(next);
    }

    fn alt_tab_select(&mut self, index: usize) {
        let now = self.clock.now();
        let Some(open) = &mut self.alt_tab.open else {
            return;
        };
        let old = open.selected;
        open.selected = index.min(open.entries.len().saturating_sub(1));
        if open.selected != old {
            open.moved = Some((old, now));
        }
        for view in &mut open.views {
            let mine = (view.first..view.first + view.count)
                .contains(&open.selected)
                .then(|| open.selected - view.first);
            view.panel.view.select(mine);
        }
        self.needs_redraw = true;
    }

    /// A key while it is open: Tab and the arrows move, Enter picks,
    /// Escape gives up.
    pub fn alt_tab_key(&mut self, sym: Keysym, modifiers: &ModifiersState, serial: Serial) {
        let Some(open) = &self.alt_tab.open else {
            return;
        };
        // Up and down move by a row of the screen showing the choice.
        let columns = open
            .views
            .iter()
            .find(|v| (v.first..v.first + v.count).contains(&open.selected))
            .map_or(1, |v| v.panel.view.columns_shown()) as isize;
        match sym {
            Keysym::Tab if modifiers.shift => self.alt_tab_step(-1),
            Keysym::ISO_Left_Tab => self.alt_tab_step(-1),
            Keysym::Tab | Keysym::Right => self.alt_tab_step(1),
            Keysym::Left => self.alt_tab_step(-1),
            Keysym::Down => self.alt_tab_step(columns),
            Keysym::Up => self.alt_tab_step(-columns),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => self.alt_tab_pick(serial),
            Keysym::Escape => {
                self.alt_tab_close(None);
            }
            _ => {}
        }
    }

    /// Whether what kept it open was let go.
    pub fn alt_tab_released(&self, modifiers: &ModifiersState) -> bool {
        match self.alt_tab.open.as_ref().and_then(|o| o.held) {
            Some(Held::Alt) => !modifiers.alt,
            Some(Held::Super) => !modifiers.logo,
            None => false,
        }
    }

    /// The choice is made: that window, focused, its desktop shown.
    pub fn alt_tab_pick(&mut self, serial: Serial) {
        let Some(selected) = self.alt_tab.open.as_ref().map(|o| o.selected) else {
            return;
        };
        let Some(id) = self.alt_tab_close(Some(selected)) else {
            return;
        };
        if let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) {
            self.focus(Some(&window), serial);
        }
    }

    /// The pointer while it is open: a click on a card picks it; elsewhere
    /// gives up.
    pub fn alt_tab_pointer(
        &mut self,
        at: Point<f64, Logical>,
        pressed: Option<bool>,
        serial: Serial,
    ) -> bool {
        let Some(open) = &self.alt_tab.open else {
            return false;
        };
        let hit = open.views.iter().find_map(|v| {
            let area = self
                .find_output(&v.output)
                .and_then(|o| self.space.output_geometry(&o))?;
            let local = at - (area.loc + v.place).to_f64();
            let card = v
                .panel
                .view
                .card_at(hocus_ui::Point::new(local.x as f32, local.y as f32))?;
            Some(v.first + card)
        });
        match (pressed, hit) {
            (None, Some(index)) => self.alt_tab_select(index),
            (Some(true), Some(index)) => {
                self.alt_tab_select(index);
                self.alt_tab_pick(serial);
            }
            (Some(true), None) => {
                self.alt_tab_close(None);
            }
            _ => {}
        }
        true
    }

    /// A pinch began with `fingers`.
    pub fn pinch_begin(&mut self, fingers: u32) {
        self.alt_tab.pinch = Some((fingers, 1.0));
    }

    pub fn pinch_update(&mut self, scale: f64) {
        if let Some((_, s)) = &mut self.alt_tab.pinch {
            *s = scale;
        }
    }

    /// A pinch ended: three fingers or more, closing in, open every
    /// screen's switcher.
    pub fn pinch_end(&mut self, cancelled: bool) {
        let Some((fingers, scale)) = self.alt_tab.pinch.take() else {
            return;
        };
        if !cancelled && fingers >= 3 && scale < 0.8 && self.alt_tab.open.is_none() {
            self.open_alt_tab(Scope::AllOutputs, None, Shown::Overview);
        }
    }

    /// The overview (#86): every screen's windows in a grid, open until a
    /// pick or Escape; asked again, it closes.
    pub fn toggle_overview(&mut self) {
        if self.alt_tab.open.is_some() {
            self.alt_tab_close(None);
        } else {
            self.open_alt_tab(Scope::AllOutputs, None, Shown::Overview);
        }
    }

    /// After each dispatch: the grids drawn, frames while the ring glides,
    /// and closed windows taken out.
    pub fn refresh_alt_tab(&mut self) {
        let now = self.clock.now();
        if let Some(closing) = &self.alt_tab.closing {
            let gone = closing
                .open
                .entries
                .iter()
                .any(|(id, _)| !self.windows.contains_key(id));
            if gone || !crate::matrix_screens::closing_runs(closing, now) {
                self.alt_tab.closing = None;
            }
            self.needs_redraw = true;
        }
        let gone = self.alt_tab.open.as_ref().is_some_and(|o| {
            o.entries
                .iter()
                .any(|(id, _)| !self.windows.contains_key(id))
        });
        if gone {
            // Opened again as the windows are now.
            self.alt_tab.open = None;
            self.needs_redraw = true;
            return;
        }
        let Some(open) = &mut self.alt_tab.open else {
            return;
        };
        for view in &mut open.views {
            let drawn = view.panel.view.draw(now);
            if view.panel.upload(&drawn) || view.panel.view.is_animating(now) {
                self.needs_redraw = true;
            }
        }
        // The previews are live.
        self.needs_redraw = true;
    }

    /// On `output`: its grid, and each card's window drawn in it, front
    /// first.
    pub fn alt_tab_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        // The Matrix theme's own, coming, there, going (#155).
        if self.matrix_look() {
            return self.matrix_switcher_elements(renderer, output);
        }
        let Some(open) = &self.alt_tab.open else {
            return Vec::new();
        };
        let Some(view) = open.views.iter().find(|v| v.output == output.name()) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let mut elements = Vec::new();
        for i in 0..view.count {
            let Some((id, _)) = open.entries.get(view.first + i) else {
                continue;
            };
            let (Some(window), Some(slot)) = (
                self.windows.get(id).map(|m| m.window.clone()),
                view.panel.view.preview(i),
            ) else {
                continue;
            };
            let slot = Rectangle::<f64, Logical>::new(
                (
                    f64::from(view.place.x) + f64::from(slot.x),
                    f64::from(view.place.y) + f64::from(slot.y),
                )
                    .into(),
                (f64::from(slot.w), f64::from(slot.h)).into(),
            );
            elements.extend(preview_elements(renderer, &window, slot, scale, 1.0));
        }
        if let Some(e) = view.panel.element(renderer, output, view.place) {
            elements.push(SceneElement::Frame(e));
        }
        elements
    }

    /// The switcher's panel on glass, under its previews.
    pub(crate) fn alt_tab_glass(&self, output: &Output) -> Vec<crate::blur::Spot> {
        let Some(open) = &self.alt_tab.open else {
            return Vec::new();
        };
        open.views
            .iter()
            .find(|v| v.output == output.name())
            .map_or_else(Vec::new, |view| {
                crate::blur::spots(
                    crate::blur::Surface::WindowSwitcher,
                    view.place.to_f64(),
                    &view.panel.view.glass(),
                )
            })
    }

    /// The cards, for the control API.
    pub fn alt_tab_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(open) = &self.alt_tab.open else {
            return;
        };
        for view in &open.views {
            if let Some(area) = self
                .find_output(&view.output)
                .and_then(|o| self.space.output_geometry(&o))
            {
                crate::shell::flatten(&view.panel.view.access(), area.loc + view.place, 0, out);
            }
        }
    }
}

/// A window drawn small in `slot` (logical, from the output's corner),
/// kept in proportion and centred, its corners rounded.
pub(crate) fn preview_elements<R>(
    renderer: &mut R,
    window: &Window,
    slot: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<SceneElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let geometry = window.geometry();
    if geometry.size.w <= 0 || geometry.size.h <= 0 {
        return Vec::new();
    }
    let k =
        (slot.size.w / f64::from(geometry.size.w)).min(slot.size.h / f64::from(geometry.size.h));
    let shown = geometry.size.to_f64().upscale(k);
    let at = slot.loc + Point::from(((slot.size.w - shown.w) / 2.0, (slot.size.h - shown.h) / 2.0));
    // Drawn at its size with its geometry's corner at `at`, then scaled
    // about that corner: an element's size follows the output's scale.
    let corner = at.to_physical_precise_round(scale);
    let origin = corner - geometry.loc.to_f64().to_physical_precise_round(scale);
    let clip = Rectangle::new(at, geometry.size.to_f64()).to_physical_precise_round(scale);
    let content: Vec<WaylandSurfaceRenderElement<R>> =
        window.render_elements(renderer, origin, Scale::from(scale), alpha);
    let radius = (6.0 * scale / k).round() as i32;
    content
        .into_iter()
        .map(|e| {
            let rounded = Rounded::new(e, clip, radius).at_scale(Scale::from(scale));
            SceneElement::Scaled(RescaleRenderElement::from_element(
                rounded,
                corner,
                Scale::from(k),
            ))
        })
        .collect()
}
