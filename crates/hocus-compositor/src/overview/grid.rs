//! Every desktop at once (issue #944, docs/design/windows.md §12c): a
//! mode of the overview, a level past it. Each screen shows its own
//! desktops in a grid, at one scale for every screen, each desktop with
//! its windows at their places and sizes; the desktop each screen shows
//! is ringed.
//!
//! `Super+Shift+W` (`[shortcuts] all-desktops`), three fingers down, the
//! top-right corner, or the palette open it, from the desktop or the
//! overview; again, `Super+W` or three fingers up (built in, whatever
//! `swipe:3:up` is bound to) zoom back into the desktops shown; `Esc` or
//! a click outside every desktop close it at once. A click on a desktop,
//! `Enter` or `Super+N` zoom into that desktop, every screen going there
//! when the screens are linked (#947), the screen alone otherwise; a
//! click on a window focuses it too. A window dragged (6 px) onto
//! another desktop goes there, of that screen, the view staying open: a
//! floating one where it is let go, a snapped one in the same zone. The
//! arrows move a selection through the grid, `Tab` into its windows.
//!
//! The zoom: every desktop's picture travels from where the strip (or
//! the overview) has it to its place in the grid, positions and scales
//! together, 320 ms on the strip's curve, from one start time for every
//! screen; zooming into a desktop is the same backwards. With reduced
//! motion, a 120 ms cross-fade. At rest nothing draws but what clients
//! commit and a waiting agent's panda (`crate::overview_draw`).

use std::time::Duration;

use smithay::{
    output::Output,
    utils::{Logical, Point},
};

use crate::{
    State,
    model::{Mode, Rect, WindowId},
    motion::{self, Bezier, Tween},
    overview::{DRAG_SLOP, Key, LIFTING, SPRING_BACK, area_of},
    switcher::Area,
};

/// The zoom between the levels: its time and the strip's curve.
pub const ZOOM: Duration = Duration::from_millis(320);
pub const ZOOM_CURVE: Bezier = Bezier::STRIP;
/// The grid in a screen's work area: 64 px in from its edges, 48 px
/// between columns, 40 between rows, 56 under each desktop for its
/// label, which stands 14 px under it.
pub const MARGIN: f64 = 64.0;
pub const COLUMN_GAP: f64 = 48.0;
pub const ROW_GAP: f64 = 40.0;
pub const LABEL_ROOM: f64 = 56.0;
pub const LABEL_GAP: f64 = 14.0;
/// A window carried grows this much about the pointer.
pub const LIFT_SCALE: f64 = 1.04;

/// What the view zoomed out from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    /// The desktops shown: each one shrinks to its place, the others come
    /// from beside it, where the strip has them.
    Desktop,
    /// The overview: its thumbnails grow into the grid, its windows go
    /// back to their places in their desktop.
    Overview,
}

/// What is under the pointer in the view.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    /// A window of a desktop: its screen, the desktop (from 0), the window.
    Window(String, usize, WindowId),
    /// A desktop (its picture or its label): its screen, the desktop.
    Desktop(String, usize),
    /// The rest of a screen.
    Outside(String),
}

impl Spot {
    /// The desktop it is on, if any: its screen and number (from 0).
    pub fn desktop(&self) -> Option<(String, usize)> {
        match self {
            Self::Window(o, d, _) | Self::Desktop(o, d) => Some((o.clone(), *d)),
            Self::Outside(_) => None,
        }
    }
}

/// A press in the view, until it is let go or moves into a carry.
#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    pub spot: Spot,
    pub at: (f64, f64),
    pub button: u32,
}

/// A window carried to another desktop.
#[derive(Debug, Clone, PartialEq)]
pub struct Carry {
    pub window: WindowId,
    pub button: u32,
    /// Its picture as it lifted (global), and since when it lifts.
    pub from: Area,
    pub since: Duration,
    /// Where the pointer is in its picture, across and down (0 to 1).
    pub grab: (f64, f64),
    /// The pointer.
    pub at: (f64, f64),
    /// The desktop it would go to, let go now: its screen and number.
    pub over: Option<(String, usize)>,
}

impl Carry {
    /// Its picture at `now`, global: under the pointer at the view's
    /// scale, grown to 1.04 about the pointer as it lifts.
    pub fn rect(&self, now: Duration, reduced: bool) -> Area {
        let at = Area {
            x: self.at.0 - self.grab.0 * self.from.w,
            y: self.at.1 - self.grab.1 * self.from.h,
            ..self.from
        };
        let k = if reduced {
            LIFT_SCALE
        } else {
            let t = Tween::new(self.since, LIFTING, Bezier::EASE_OUT).at(now);
            motion::lerp(1.0, LIFT_SCALE, t)
        };
        at.scaled(k, self.at)
    }
}

/// The keyboard's selection.
#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    /// None shows yet: the first arrow starts from the desktop shown.
    None,
    /// A desktop: the screen it was chosen on, its number (from 0).
    Desktop(String, usize),
    /// One of that desktop's windows (`Tab`).
    Window(String, usize, WindowId),
}

/// The view while it shows.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Since when it zooms out, and from what.
    pub since: Duration,
    pub from: From,
    /// The desktop the pointer is over: its screen and number.
    pub hover: Option<(String, usize)>,
    pub press: Option<Press>,
    pub carry: Option<Carry>,
    /// A window let go nowhere, springing back from there to its place.
    pub back: Option<(WindowId, Area, Tween)>,
    pub selection: Selection,
}

impl Grid {
    pub fn new(since: Duration, from: From) -> Self {
        Self {
            since,
            from,
            hover: None,
            press: None,
            carry: None,
            back: None,
            selection: Selection::None,
        }
    }

    /// The pointer's press, carry and hover end.
    pub fn let_go(&mut self) {
        self.press = None;
        self.carry = None;
        self.hover = None;
    }
}

/// How a grid of `count` desktops is cut: columns and rows, in reading
/// order. One row for 1 or 2, 2 × 2 for 3 or 4, 3 × 2 for 5 or 6, 3 × 3
/// for 7 to 9; past 9, rows of 4.
pub fn shape(count: usize) -> (usize, usize) {
    let count = count.max(1);
    let cols = match count {
        1 | 2 => count,
        3 | 4 => 2,
        5..=9 => 3,
        _ => 4,
    };
    (cols, count.div_ceil(cols))
}

/// The largest scale at which a `cols` × `rows` grid of a screen of
/// `screen`'s size fits `work` (the screen but its bar's column), its
/// margins, gaps and labels.
pub fn fit(screen: Area, work: Area, (cols, rows): (usize, usize)) -> f64 {
    let (c, r) = (cols as f64, rows as f64);
    let room_w = work.w - 2.0 * MARGIN - (c - 1.0) * COLUMN_GAP;
    let room_h = work.h - 2.0 * MARGIN - (r - 1.0) * ROW_GAP - r * LABEL_ROOM;
    let k = (room_w / (c * screen.w)).min(room_h / (r * screen.h));
    k.max(0.01)
}

/// The places of a screen's `count` desktops at scale `k`, global: in
/// reading order, the grid centred in the work area.
pub fn places(screen: Area, work: Area, count: usize, shape: (usize, usize), k: f64) -> Vec<Area> {
    let (cols, rows) = shape;
    let (w, h) = (screen.w * k, screen.h * k);
    let total_w = cols as f64 * w + (cols as f64 - 1.0) * COLUMN_GAP;
    let total_h = rows as f64 * (h + LABEL_ROOM) + (rows as f64 - 1.0) * ROW_GAP;
    let x0 = work.x + (work.w - total_w) / 2.0;
    let y0 = work.y + (work.h - total_h) / 2.0;
    (0..count)
        .map(|i| Area {
            x: x0 + (i % cols) as f64 * (w + COLUMN_GAP),
            y: y0 + (i / cols) as f64 * (h + LABEL_ROOM + ROW_GAP),
            w,
            h,
        })
        .collect()
}

/// Where a window at `rect` on a screen at `screen` is drawn in a
/// desktop's picture at `picture`.
pub fn inside(rect: Rect, screen: Area, picture: Area) -> Area {
    let k = picture.w / screen.w.max(1.0);
    Area {
        x: picture.x + (f64::from(rect.x) - screen.x) * k,
        y: picture.y + (f64::from(rect.y) - screen.y) * k,
        w: f64::from(rect.width) * k,
        h: f64::from(rect.height) * k,
    }
}

/// The desktop the selection goes to from `from` by (`dx`, `dy`) in a
/// grid of `count` desktops `cols` wide: across in reading order, up and
/// down by rows; nothing past the first or the last.
pub fn step(from: usize, (dx, dy): (i32, i32), count: usize, cols: usize) -> usize {
    let by = i64::from(dx) + i64::from(dy) * cols as i64;
    let to = from as i64 + by;
    if (0..count as i64).contains(&to) {
        to as usize
    } else {
        from
    }
}

/// What a desktop's label says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// Its number, and its name if it has one: "3 · Courrier".
    Number(String),
    /// A project's: its monogram, "2 · hocus-site", its folder.
    Project {
        mono: String,
        title: String,
        folder: String,
    },
    /// Under a window carried: "Lâcher : bureau 3".
    Drop(String),
}

/// A screen in the view: its place, its work area, its desktops' places.
#[derive(Debug, Clone)]
pub struct Screen {
    pub output: Output,
    pub geometry: Area,
    pub places: Vec<Area>,
}

impl State {
    /// Whether every desktop shows and takes the pointer and the keys.
    pub fn all_desktops_open(&self) -> bool {
        self.overview
            .as_ref()
            .is_some_and(|o| o.open() && o.grid.is_some())
    }

    /// `Super+Shift+W`, the top-right corner: every desktop, from the
    /// desktop or the overview; open, back to the desktops shown.
    pub fn toggle_all_desktops(&mut self) {
        if self.all_desktops_open() {
            self.close_overview(None);
        } else {
            self.open_all_desktops();
        }
    }

    /// Every desktop, from the desktop or the overview (three fingers
    /// down); nothing when it shows already.
    pub fn open_all_desktops(&mut self) {
        if self.all_desktops_open() {
            return;
        }
        let now = self.now();
        if self.overview_open() {
            // From the overview: its thumbnails grow into the grid.
            self.tell_shell(hocus_link::ToShell::ProjectEnd);
            if let Some(overview) = &mut self.overview {
                overview.press = None;
                overview.drag = None;
                overview.hover = None;
                overview.back = None;
                overview.grid = Some(Grid::new(now, From::Overview));
            }
            tracing::info!("every desktop shows, from the overview");
            self.redraw = true;
            return;
        }
        if !self.open_overview_as(Some(Grid::new(now, From::Desktop))) {
            return;
        }
        tracing::info!("every desktop shows");
    }

    /// Gone at once (`Esc`, a click outside every desktop, a screen
    /// plugged or unplugged): the desktops shown as they were.
    pub fn close_all_desktops_at_once(&mut self) {
        if !self
            .overview
            .as_ref()
            .is_some_and(|o| o.grid.is_some() && o.open())
        {
            return;
        }
        tracing::info!("every desktop closes at once");
        self.overview = None;
        self.redraw = true;
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            let time = self.input_time();
            self.pointer_motion(at, time);
        }
    }

    /// The screens, left to right, with their desktops' places: one grid
    /// for every screen (the most desktops any has), at the one scale at
    /// which every screen's fits.
    pub fn grid_screens(&self) -> Vec<Screen> {
        let mut outputs: Vec<Output> = self.space.outputs().cloned().collect();
        outputs.sort_by_key(|o| {
            self.space
                .output_geometry(o)
                .map_or((0, 0), |g| (g.loc.x, g.loc.y))
        });
        let screens: Vec<(Output, Area, Area, usize)> = outputs
            .into_iter()
            .filter_map(|output| {
                let geometry = area_of(crate::windows::rect_of(
                    self.space.output_geometry(&output)?,
                ));
                // The screen, but the bar's side: from the bar's edge (its
                // column stays empty), else from the screen's edges.
                let work = self.work_area(&output).map_or(geometry, area_of);
                let room = Area {
                    x: work.x,
                    w: work.w,
                    ..geometry
                };
                let count = self.strip(&output).count();
                Some((output, geometry, room, count))
            })
            .collect();
        let most = screens.iter().map(|s| s.3).max().unwrap_or(1);
        let cut = shape(most);
        let k = screens
            .iter()
            .map(|(_, geometry, work, _)| fit(*geometry, *work, cut))
            .fold(f64::INFINITY, f64::min);
        screens
            .into_iter()
            .map(|(output, geometry, work, count)| Screen {
                output,
                geometry,
                places: places(geometry, work, count, cut, k),
            })
            .collect()
    }

    /// The grid's columns.
    fn grid_columns(&self) -> usize {
        let most = self
            .space
            .outputs()
            .map(|o| self.strip(o).count())
            .max()
            .unwrap_or(1);
        shape(most).0
    }

    /// The windows desktop `desktop` of `output` shows in the view, back
    /// to front: those on it or on every desktop, not minimised; the one
    /// carried out.
    pub fn grid_windows(&self, output: &Output, desktop: usize) -> Vec<WindowId> {
        let carried = self
            .overview
            .as_ref()
            .and_then(|o| o.grid.as_ref())
            .and_then(|g| g.carry.as_ref())
            .map(|c| c.window);
        self.windows_on(output)
            .into_iter()
            .filter(|id| Some(*id) != carried)
            .filter(|id| !self.model.is_minimized(*id))
            .filter(|id| self.model.desktop(*id) == desktop || self.model.is_everywhere(*id))
            .collect()
    }

    /// Window `id`'s picture in `screen`'s desktop `desktop`, at rest.
    pub fn grid_window_rect(&self, screen: &Screen, desktop: usize, id: WindowId) -> Option<Area> {
        let picture = *screen.places.get(desktop)?;
        Some(inside(self.home_rect(id)?, screen.geometry, picture))
    }

    /// What desktop `desktop` of `output`'s label says: under a window
    /// carried over it (on its screen, or on every screen when linked),
    /// what letting go does.
    pub fn grid_label(&self, output: &Output, desktop: usize) -> Label {
        let lang = self.radial_lang();
        let dropping = self
            .overview
            .as_ref()
            .and_then(|o| o.grid.as_ref())
            .and_then(|g| g.carry.as_ref())
            .and_then(|c| c.over.as_ref())
            .is_some_and(|(o, d)| *d == desktop && (self.desktops_linked() || *o == output.name()));
        if dropping {
            return Label::Drop(hocus_words::compositor::overview_draw::drop_on_desktop(
                lang,
                desktop + 1,
            ));
        }
        if let Some(project) = self.project_on(output, desktop) {
            let home = self
                .config
                .home
                .as_ref()
                .map(|h| h.display().to_string())
                .unwrap_or_default();
            return Label::Project {
                mono: hocus_link::projects::monogram(&project.name),
                title: format!("{} · {}", desktop + 1, project.name),
                folder: hocus_link::projects::tilde(&project.folder.display().to_string(), &home),
            };
        }
        let names = self.desktop_names_on(output);
        Label::Number(match names.get(desktop).filter(|n| !n.is_empty()) {
            Some(n) => format!("{} · {n}", desktop + 1),
            None => format!("{}", desktop + 1),
        })
    }

    /// What is under `at` (global) in the view.
    pub fn grid_spot(&self, at: (f64, f64)) -> Option<Spot> {
        let screens = self.grid_screens();
        let screen = screens.iter().find(|s| s.geometry.contains(at.0, at.1))?;
        let name = screen.output.name();
        for (desktop, place) in screen.places.iter().enumerate() {
            let with_label = Area {
                h: place.h + LABEL_ROOM,
                ..*place
            };
            if !with_label.contains(at.0, at.1) {
                continue;
            }
            if place.contains(at.0, at.1) {
                let front = self
                    .grid_windows(&screen.output, desktop)
                    .into_iter()
                    .rev()
                    .find(|id| {
                        self.grid_window_rect(screen, desktop, *id)
                            .is_some_and(|r| r.contains(at.0, at.1))
                    });
                if let Some(id) = front {
                    return Some(Spot::Window(name, desktop, id));
                }
            }
            return Some(Spot::Desktop(name, desktop));
        }
        Some(Spot::Outside(name))
    }

    /// The pointer moved in the view.
    pub(super) fn grid_motion(&mut self, at: Point<f64, Logical>) {
        let now = self.now();
        let spot = self.grid_spot((at.x, at.y));
        let lifting = self
            .overview
            .as_ref()
            .and_then(|o| o.grid.as_ref())
            .and_then(|g| g.press.clone())
            .and_then(|press| match &press.spot {
                Spot::Window(output, desktop, id)
                    if (at.x - press.at.0).hypot(at.y - press.at.1) >= DRAG_SLOP =>
                {
                    let screen = self
                        .grid_screens()
                        .into_iter()
                        .find(|s| s.output.name() == *output)?;
                    let (desktop, id) = (*desktop, *id);
                    let from = self.grid_window_rect(&screen, desktop, id)?;
                    Some((press.clone(), id, from))
                }
                _ => None,
            });
        let Some(grid) = self.overview.as_mut().and_then(|o| o.grid.as_mut()) else {
            return;
        };
        if let Some((press, id, from)) = lifting {
            tracing::debug!(window = id.0, "a window lifts in every desktop");
            grid.carry = Some(Carry {
                window: id,
                button: press.button,
                from,
                since: now,
                grab: (
                    ((press.at.0 - from.x) / from.w.max(1.0)).clamp(0.0, 1.0),
                    ((press.at.1 - from.y) / from.h.max(1.0)).clamp(0.0, 1.0),
                ),
                at: (at.x, at.y),
                over: None,
            });
            grid.press = None;
            grid.back = None;
        }
        let over = spot.as_ref().and_then(Spot::desktop);
        if let Some(carry) = &mut grid.carry {
            carry.at = (at.x, at.y);
            carry.over = over;
            grid.hover = None;
            self.redraw = true;
            return;
        }
        if grid.hover != over {
            grid.hover = over;
            self.redraw = true;
        }
    }

    /// A press in the view.
    pub(super) fn grid_press(&mut self, button: u32, at: Point<f64, Logical>) {
        let spot = self.grid_spot((at.x, at.y));
        let Some(grid) = self.overview.as_mut().and_then(|o| o.grid.as_mut()) else {
            return;
        };
        if grid.press.is_some() || grid.carry.is_some() {
            return;
        }
        if let Some(spot) = spot {
            grid.press = Some(Press {
                spot,
                at: (at.x, at.y),
                button,
            });
        }
    }

    /// A button let go in the view: whether it was the view's.
    pub(super) fn grid_release(&mut self, button: u32, at: Point<f64, Logical>) -> bool {
        let Some(grid) = self.overview.as_mut().and_then(|o| o.grid.as_mut()) else {
            return false;
        };
        if let Some(carry) = grid.carry.take_if(|c| c.button == button) {
            self.grid_drop(carry, at);
            return true;
        }
        let Some(press) = grid.press.take_if(|p| p.button == button) else {
            return false;
        };
        if button != crate::grabs::BTN_LEFT {
            return true;
        }
        match press.spot {
            Spot::Window(output, desktop, id) => self.grid_go(&output, desktop, Some(id)),
            Spot::Desktop(output, desktop) => self.grid_go(&output, desktop, None),
            Spot::Outside(_) => self.close_all_desktops_at_once(),
        }
        true
    }

    /// Zooms into desktop `desktop` of `output`: every screen goes there
    /// when they are linked (#947), `output` alone otherwise; every
    /// screen's view zooms into the desktop it then shows. `focus`, a
    /// window there, gets the keyboard; else the focus goes as after a
    /// switch.
    pub fn grid_go(&mut self, output: &str, desktop: usize, focus: Option<WindowId>) {
        let Some(output) = self.space.outputs().find(|o| o.name() == output).cloned() else {
            return;
        };
        if desktop >= self.strip(&output).count() {
            return;
        }
        tracing::info!(
            output = output.name(),
            desktop = desktop + 1,
            window = focus.map(|f| f.0),
            "every desktop zooms into one"
        );
        self.switch_desktop_at_once(&output, desktop);
        if let Some(id) = focus.filter(|id| self.model.is_shown(*id)) {
            self.focus_raising(Some(id), true);
        }
        self.close_overview(None);
    }

    /// The screen the keyboard acts on: the pointer's, else the first.
    fn grid_screen(&self) -> Option<Output> {
        self.output_under_pointer()
            .or_else(|| self.space.outputs().next().cloned())
    }

    /// A key while every desktop shows.
    pub(super) fn grid_key(&mut self, key: Key) {
        let selection = self
            .overview
            .as_ref()
            .and_then(|o| o.grid.as_ref())
            .map_or(Selection::None, |g| g.selection.clone());
        match key {
            Key::Nothing => {}
            Key::Escape => match selection {
                Selection::Window(output, desktop, _) => {
                    self.grid_select(Selection::Desktop(output, desktop));
                }
                _ => self.close_all_desktops_at_once(),
            },
            Key::Close | Key::AllDesktops => self.close_overview(None),
            Key::Step(dx, dy) => {
                let Some(screen) = self.grid_screen() else {
                    return;
                };
                let (output, from) = match selection {
                    Selection::Desktop(o, d) | Selection::Window(o, d, _) => (o, Some(d)),
                    Selection::None => (screen.name(), None),
                };
                let Some(on) = self.space.outputs().find(|o| o.name() == output).cloned() else {
                    return;
                };
                let strip = self.strip(&on);
                let to = match from {
                    // The first arrow starts from the desktop shown.
                    None => step(
                        strip.current(),
                        (dx, dy),
                        strip.count(),
                        self.grid_columns(),
                    ),
                    Some(d) => step(d, (dx, dy), strip.count(), self.grid_columns()),
                };
                self.grid_select(Selection::Desktop(output, to));
            }
            Key::Enter => match selection {
                Selection::Window(output, desktop, id) => {
                    self.grid_go(&output, desktop, Some(id));
                }
                Selection::Desktop(output, desktop) => self.grid_go(&output, desktop, None),
                Selection::None => self.close_overview(None),
            },
            Key::Show(desktop) => {
                let output = match selection {
                    Selection::Desktop(o, _) | Selection::Window(o, _, _) => Some(o),
                    Selection::None => self.grid_screen().map(|o| o.name()),
                };
                if let Some(output) = output {
                    self.grid_go(&output, desktop, None);
                }
            }
            Key::Send(desktop) => {
                let Selection::Window(output, from, id) = selection else {
                    return;
                };
                let Some(on) = self.output_of(id) else {
                    return;
                };
                if desktop >= self.strip(&on).count() || desktop == self.model.desktop(id) {
                    return;
                }
                let next = self
                    .grid_tab_order(from)
                    .into_iter()
                    .find(|(_, w)| *w != id);
                tracing::info!(
                    window = id.0,
                    desktop = desktop + 1,
                    "every desktop sends a window"
                );
                self.grid_place(id, &on, desktop, None);
                self.grid_select(match next {
                    Some((o, w)) => Selection::Window(o, from, w),
                    None => Selection::Desktop(output, from),
                });
            }
            Key::Tab(forward) => {
                let (desktop, current) = match &selection {
                    Selection::Desktop(o, d) => (Some((o.clone(), *d)), None),
                    Selection::Window(o, d, w) => (Some((o.clone(), *d)), Some(*w)),
                    Selection::None => (None, None),
                };
                let (output, desktop) = match desktop {
                    Some(d) => d,
                    None => {
                        let Some(screen) = self.grid_screen() else {
                            return;
                        };
                        let shown = self.strip(&screen).current();
                        (screen.name(), shown)
                    }
                };
                let order = self.grid_tab_order(desktop);
                if order.is_empty() {
                    self.grid_select(Selection::Desktop(output, desktop));
                    return;
                }
                let at = current.and_then(|w| order.iter().position(|(_, id)| *id == w));
                let n = order.len();
                let next = match (at, forward) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                };
                if let Some((_, id)) = order.get(next) {
                    self.grid_select(Selection::Window(output, desktop, *id));
                }
            }
        }
    }

    /// Desktop `desktop`'s windows on every screen, the screens left to
    /// right, each one's front first: `Tab`'s order. Each with its screen.
    fn grid_tab_order(&self, desktop: usize) -> Vec<(String, WindowId)> {
        self.grid_screens()
            .iter()
            .filter(|s| desktop < s.places.len())
            .flat_map(|s| {
                let name = s.output.name();
                self.grid_windows(&s.output, desktop)
                    .into_iter()
                    .rev()
                    .map(move |id| (name.clone(), id))
            })
            .collect()
    }

    fn grid_select(&mut self, selection: Selection) {
        if let Some(grid) = self.overview.as_mut().and_then(|o| o.grid.as_mut())
            && grid.selection != selection
        {
            grid.selection = selection;
            self.redraw = true;
        }
    }

    /// A window carried let go at `at`: onto another desktop, it goes
    /// there, of that screen; elsewhere it springs back.
    fn grid_drop(&mut self, carry: Carry, at: Point<f64, Logical>) {
        let id = carry.window;
        let now = self.now();
        let reduced = self.reduced_motion();
        let picture = carry.rect(now, reduced);
        let home = self.output_of(id);
        // A desktop's picture, or its tile in the bar (8.5 B).
        let on_tile = self.tile_under(at).map(|o| (o.output, o.desktop));
        let dropped_on_picture = on_tile.is_none();
        let target = on_tile
            .or_else(|| self.grid_spot((at.x, at.y)).and_then(|s| s.desktop()))
            .and_then(|(output, desktop)| {
                let to = self.space.outputs().find(|o| o.name() == output).cloned()?;
                let own = home.as_ref() == Some(&to)
                    && (self.model.desktop(id) == desktop || self.model.is_everywhere(id));
                (!own).then_some((to, desktop))
            });
        let moved = match target {
            Some((to, desktop)) => {
                let screen = self.grid_screens().into_iter().find(|s| s.output == to);
                let dropped = screen
                    .as_ref()
                    .filter(|_| dropped_on_picture)
                    .and_then(|s| {
                        let place = *s.places.get(desktop)?;
                        let k = place.w / s.geometry.w.max(1.0);
                        Some((
                            s.geometry.x + (picture.x - place.x) / k,
                            s.geometry.y + (picture.y - place.y) / k,
                        ))
                    });
                tracing::info!(
                    window = id.0,
                    output = to.name(),
                    desktop = desktop + 1,
                    "every desktop takes a window to another desktop"
                );
                self.grid_place(id, &to, desktop, dropped);
                true
            }
            None => false,
        };
        if let Some(grid) = self.overview.as_mut().and_then(|o| o.grid.as_mut()) {
            grid.back = (!moved).then(|| {
                (
                    id,
                    picture,
                    Tween::new(now, SPRING_BACK, motion::Bezier::SPRING),
                )
            });
            grid.hover = None;
        }
        self.redraw = true;
    }

    /// Puts window `id` on desktop `desktop` of `to`, on top of its
    /// windows, nothing switching: floating, its top-left at `dropped`
    /// (global, else where it is on its screen), its size kept but no
    /// larger than the work area, wholly inside it; snapped, maximised or
    /// fullscreen, the same zone there.
    fn grid_place(
        &mut self,
        id: WindowId,
        to: &Output,
        desktop: usize,
        dropped: Option<(f64, f64)>,
    ) {
        let Some(from) = self.output_of(id) else {
            return;
        };
        let (Some(area), Some(to_area), Some(to_geometry), Some(rect)) = (
            self.work_area(&from),
            self.work_area(to),
            self.space.output_geometry(to).map(crate::windows::rect_of),
            self.home_rect(id),
        ) else {
            return;
        };
        let desktop = desktop.min(self.strip(to).count().saturating_sub(1));
        let placed = match self.model.mode(id) {
            Mode::Floating => {
                let width = rect.width.min(to_area.width);
                let height = rect.height.min(to_area.height);
                let (x, y) = dropped.map_or_else(
                    || {
                        let from_geometry = self
                            .space
                            .output_geometry(&from)
                            .map_or((0, 0), |g| (g.loc.x, g.loc.y));
                        (
                            to_geometry.x + rect.x - from_geometry.0,
                            to_geometry.y + rect.y - from_geometry.1,
                        )
                    },
                    |(x, y)| (x.round() as i32, y.round() as i32),
                );
                Rect {
                    x: x.clamp(to_area.x, to_area.x + to_area.width - width),
                    y: y.clamp(to_area.y, to_area.y + to_area.height - height),
                    width,
                    height,
                }
            }
            Mode::Snapped(zone) => zone.rect(to_area, self.gap()),
            Mode::Fullscreen => to_geometry,
        };
        let restore = self
            .model
            .placement(id)
            .restore
            .map(|r| crate::model::carried(r, area, to_area));
        if self.model.is_everywhere(id) {
            self.set_everywhere(id, false);
        }
        let had_focus = self.model.focused() == Some(id);
        self.relocate(id, to, desktop, placed, restore);
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.came_from = None;
        }
        // On top of that desktop's windows (#728), which focuses it when
        // shown (#888).
        self.model.raise(id);
        self.desktop_focus.insert((to.name(), desktop), id);
        self.restack();
        if had_focus && !self.model.is_shown(id) {
            let next =
                self.model.stack().iter().rev().copied().find(|w| {
                    self.model.is_shown(*w) && self.output_of(*w).as_ref() == Some(&from)
                });
            self.focus(next);
        }
        self.rearrange();
        self.session_changed();
        self.redraw = true;
    }

    /// A window went: the view forgets it.
    pub(super) fn grid_window_gone(&mut self, id: WindowId) {
        let Some(grid) = self.overview.as_mut().and_then(|o| o.grid.as_mut()) else {
            return;
        };
        if grid.carry.as_ref().is_some_and(|c| c.window == id) {
            grid.carry = None;
        }
        if grid.back.is_some_and(|(w, _, _)| w == id) {
            grid.back = None;
        }
        if matches!(&grid.press, Some(Press { spot: Spot::Window(_, _, w), .. }) if *w == id) {
            grid.press = None;
        }
        if let Selection::Window(o, d, w) = &grid.selection
            && *w == id
        {
            grid.selection = Selection::Desktop(o.clone(), *d);
        }
    }

    /// A screen plugged or unplugged: the view closes at once.
    pub(crate) fn grid_screens_changed(&mut self) {
        self.close_all_desktops_at_once();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_s_shape_by_the_number_of_desktops() {
        assert_eq!(shape(1), (1, 1));
        assert_eq!(shape(2), (2, 1));
        assert_eq!(shape(3), (2, 2));
        assert_eq!(shape(4), (2, 2));
        assert_eq!(shape(5), (3, 2));
        assert_eq!(shape(6), (3, 2));
        assert_eq!(shape(7), (3, 3));
        assert_eq!(shape(9), (3, 3));
        assert_eq!(shape(10), (4, 3));
    }

    #[test]
    fn one_scale_for_the_laptop_and_the_34_inch() {
        // The laptop: 2048 × 1280 logical, its bar's column on the right;
        // the 34-inch: 3440 × 1440.
        let laptop = Area {
            x: 0.0,
            y: 0.0,
            w: 2048.0,
            h: 1280.0,
        };
        let laptop_work = Area {
            w: 2048.0 - 236.0,
            ..laptop
        };
        let wide = Area {
            x: 2048.0,
            y: 0.0,
            w: 3440.0,
            h: 1440.0,
        };
        let k = fit(laptop, laptop_work, (2, 2)).min(fit(wide, wide, (2, 2)));
        assert!((k - 0.390625).abs() < 1e-9, "{k}");
        let small = places(laptop, laptop_work, 4, (2, 2), k);
        assert!((small[0].w - 800.0).abs() < 1e-9 && (small[0].h - 500.0).abs() < 1e-9);
        let large = places(wide, wide, 4, (2, 2), k);
        assert!((large[0].w - 1343.75).abs() < 1e-9 && (large[0].h - 562.5).abs() < 1e-9);
        // In reading order: 2 right of 1, 3 under 1.
        assert!(small[1].x > small[0].x && small[1].y == small[0].y);
        assert!(small[2].x == small[0].x && small[2].y > small[0].y);
        // Centred in the work area.
        let middle = (small[0].x + small[1].x + small[1].w) / 2.0;
        assert!((middle - (laptop_work.x + laptop_work.w / 2.0)).abs() < 1e-9);
        assert!(small[0].y >= MARGIN - 1e-9);
    }

    #[test]
    fn the_arrows_go_in_reading_order_and_by_rows_never_past_the_ends() {
        assert_eq!(step(1, (1, 0), 4, 2), 2);
        assert_eq!(step(0, (0, 1), 4, 2), 2);
        assert_eq!(step(2, (0, -1), 4, 2), 0);
        assert_eq!(step(3, (1, 0), 4, 2), 3);
        assert_eq!(step(0, (-1, 0), 4, 2), 0);
        assert_eq!(step(2, (0, 1), 4, 2), 2);
        // 8 in a 3 × 3: under 6, nothing.
        assert_eq!(step(6, (0, 1), 8, 3), 6);
        assert_eq!(step(4, (0, 1), 8, 3), 7);
    }

    #[test]
    fn a_window_inside_its_desktop_s_picture() {
        let screen = Area {
            x: 100.0,
            y: 0.0,
            w: 1000.0,
            h: 500.0,
        };
        let picture = Area {
            x: 10.0,
            y: 20.0,
            w: 500.0,
            h: 250.0,
        };
        let rect = Rect {
            x: 300,
            y: 100,
            width: 400,
            height: 200,
        };
        let at = inside(rect, screen, picture);
        assert_eq!((at.x, at.y, at.w, at.h), (110.0, 70.0, 200.0, 100.0));
    }
}
