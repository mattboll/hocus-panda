//! The overview (8.5 A, issue #490; docs/design/windows.md §12b): every
//! screen at once, each with its desktops as live thumbnails along its top
//! and the windows of the desktop it shows laid out without overlap.
//!
//! `Super+W` (`[shortcuts] overview`), four fingers down on a touchpad
//! while no launcher is open (`crate::gestures`), or the bar's "+ N
//! autres" (the shell's `overview`) open it; the same, `Esc`, or a click
//! on the empty background close it. A click on a window focuses it and
//! closes the overview; a click on a desktop's thumbnail shows that
//! desktop and keeps it open; a window dragged (6 px) lifts to 42 % under
//! the pointer and, let go on a thumbnail or another screen, goes there.
//! The arrows move a selection among the windows, `Enter` focuses it,
//! `Super+1…9` shows a desktop, `Super+Shift+1…9` sends the selected
//! window there.
//!
//! The windows glide from their places to the layout (240 ms) and back;
//! with reduced motion the overview fades in and out (120 ms). While it is
//! open no client sees the pointer or the keys but the bar, which stays
//! in front. Windows hosting agents carry a panda badge; empty desktops'
//! thumbnails a sleeping panda (`crate::overview_draw`). At rest nothing
//! draws but what clients commit: the sleeping pandas take two breaths as
//! it opens, then lie still; only a waiting agent's panda waves on.

use std::{collections::HashMap, time::Duration};

use hocus_link::AgentShown;
use smithay::{
    input::{keyboard::Keysym, pointer::MotionEvent},
    output::Output,
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::{
    State,
    model::WindowId,
    motion::{self, Bezier, Tween},
    switcher::Area,
};

/// The thumbnails: 48 px from the screen's top, a ninth of its width
/// each, 40 px apart; their number 6 px under them on a 14 px line.
pub const STRIP_TOP: f64 = 48.0;
pub const THUMB_SHARE: f64 = 1.0 / 9.0;
pub const THUMB_GAP: f64 = 40.0;
pub const NUMBER_GAP: f64 = 6.0;
pub const NUMBER_LINE: f64 = 14.0;
pub const NUMBER_SIZE: f64 = 10.5;
/// The windows' area: 40 px under the numbers, 48 px in from the work
/// area's sides and bottom; 32 px between windows; their title 10 px
/// under each on a 20 px line, 14 px.
pub const LAYOUT_TOP: f64 = 40.0;
pub const LAYOUT_MARGIN: f64 = 48.0;
pub const WINDOW_GAP: f64 = 32.0;
pub const LABEL_GAP: f64 = 10.0;
pub const LABEL_LINE: f64 = 20.0;
pub const LABEL_SIZE: f64 = 14.0;
/// A dragged window's share of its size, under the pointer.
pub const LIFT: f64 = 0.42;
/// Where a lifted window hangs from the pointer: this share of its width
/// left of it, this far under it.
pub const HANG: (f64, f64) = (0.12, 14.0);
/// How far a press moves before it drags.
pub const DRAG_SLOP: f64 = 6.0;
/// Minimised windows show this much.
pub const MINIMIZED: f32 = 0.6;
/// The ✕ on a window under the pointer.
pub const CLOSE_BUTTON: f64 = 24.0;
/// The background under the layout: the wallpaper dimmed.
pub const DIM: [f32; 4] = [7.0 / 255.0, 9.0 / 255.0, 15.0 / 255.0, 0.5];

/// The windows glide to the layout and back in this time, along this
/// curve; with reduced motion the overview fades in this time.
pub const GLIDE: Duration = Duration::from_millis(240);
pub const GLIDE_CURVE: Bezier = Bezier(0.2, 0.9, 0.25, 1.0);
pub const FADE: Duration = Duration::from_millis(120);
/// A dragged window lifts in this time; let go nowhere, it springs back
/// in this one.
pub const LIFTING: Duration = Duration::from_millis(120);
pub const SPRING_BACK: Duration = Duration::from_millis(200);
/// A waiting agent's panda bobs and waves on this loop; a sleeping one
/// breathes on this one, for this long after the overview opens.
pub const WAVE: Duration = Duration::from_millis(1600);
pub const BREATH: Duration = Duration::from_millis(3200);
pub const BREATHING: Duration = Duration::from_millis(6400);

/// The agents of a window, by the state that needs the user most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AgentState {
    Waiting,
    Failed,
    Done,
    Working,
    Idle,
}

impl AgentState {
    pub fn of(word: &str) -> Self {
        match word {
            "waiting" => Self::Waiting,
            "failed" => Self::Failed,
            "done" => Self::Done,
            "working" => Self::Working,
            _ => Self::Idle,
        }
    }

    /// Its colour (bar.md's rows): amber, orange, green, the accent, grey.
    pub fn colour(self) -> [u8; 4] {
        match self {
            Self::Waiting => [0xF5, 0xB4, 0x4A, 0xFF],
            Self::Failed => [0xF2, 0x94, 0x5B, 0xFF],
            Self::Done => [0x7E, 0xE0, 0xA0, 0xFF],
            Self::Working => [0x6C, 0xC2, 0x87, 0xFF],
            Self::Idle => [0xA9, 0xB2, 0xC2, 0xFF],
        }
    }

    /// Its words' colour on the badge.
    pub fn ink(self) -> [u8; 4] {
        match self {
            Self::Waiting => [0xF5, 0xC4, 0x6E, 0xFF],
            Self::Failed => [0xF7, 0xB9, 0x8A, 0xFF],
            Self::Done => [0x9E, 0xEA, 0xB8, 0xFF],
            Self::Working => [0xA9, 0xDE, 0xB9, 0xFF],
            Self::Idle => [0xC9, 0xD0, 0xDC, 0xFF],
        }
    }
}

/// A window's agents: how many need the user most, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Badge {
    pub state: AgentState,
    pub count: usize,
}

impl Badge {
    /// What it says: "3 t'attendent", "3 waiting".
    pub fn words(self, french: bool) -> String {
        let n = self.count;
        let word = match (self.state, french, n > 1) {
            (AgentState::Waiting, true, false) => "t’attend",
            (AgentState::Waiting, true, true) => "t’attendent",
            (AgentState::Waiting, false, _) => "waiting",
            (AgentState::Failed, true, false) => "bloqué",
            (AgentState::Failed, true, true) => "bloqués",
            (AgentState::Failed, false, _) => "stuck",
            (AgentState::Done, true, _) => "à voir",
            (AgentState::Done, false, _) => "to see",
            (AgentState::Working, true, _) => "au travail",
            (AgentState::Working, false, _) => "working",
            (AgentState::Idle, true, _) => "au repos",
            (AgentState::Idle, false, _) => "resting",
        };
        format!("{n} {word}")
    }
}

/// The badges of the agents' windows: for each window, its agents in the
/// state that needs the user most (waiting, stuck, to see, working,
/// resting) and how many are in it.
pub fn badges(agents: &[AgentShown]) -> HashMap<WindowId, Badge> {
    let mut out: HashMap<WindowId, Badge> = HashMap::new();
    for agent in agents.iter().filter(|a| a.window != 0) {
        let state = AgentState::of(&agent.state);
        out.entry(WindowId(agent.window))
            .and_modify(|b| {
                if state < b.state {
                    *b = Badge { state, count: 1 };
                } else if state == b.state {
                    b.count += 1;
                }
            })
            .or_insert(Badge { state, count: 1 });
    }
    out
}

/// The thumbnails of a screen's `count` desktops, global: a ninth of the
/// screen's width each (smaller when they would not fit the work area),
/// the screen's shape, 40 px apart, centred over the work area, 48 px
/// from the screen's top.
pub fn thumbs(screen: Area, work: Area, count: usize) -> Vec<Area> {
    if count == 0 || screen.w <= 0.0 || screen.h <= 0.0 {
        return Vec::new();
    }
    let n = count as f64;
    let room = (work.w - 2.0 * THUMB_GAP).max(1.0);
    let w = (screen.w * THUMB_SHARE).min((room - (n - 1.0) * THUMB_GAP) / n);
    let w = w.max(8.0);
    let h = w * screen.h / screen.w;
    let total = n * w + (n - 1.0) * THUMB_GAP;
    let x0 = work.x + (work.w - total) / 2.0;
    let y = (screen.y + STRIP_TOP).max(work.y + 8.0);
    (0..count)
        .map(|i| Area {
            x: x0 + i as f64 * (w + THUMB_GAP),
            y,
            w,
            h,
        })
        .collect()
}

/// Where a screen's windows are laid out, under its thumbnails.
pub fn windows_area(work: Area, thumbs: &[Area]) -> Area {
    let top = thumbs.first().map_or(work.y, |t| {
        t.y + t.h + NUMBER_GAP + NUMBER_LINE + LAYOUT_TOP
    });
    let x = work.x + LAYOUT_MARGIN;
    let w = (work.w - 2.0 * LAYOUT_MARGIN).max(1.0);
    let h = (work.y + work.h - LAYOUT_MARGIN - top).max(1.0);
    Area { x, y: top, w, h }
}

/// Windows of these sizes laid out in `area`, in their order, row after
/// row: never larger than they are, none over another, each with room for
/// its title under it. As many rows as make them the largest.
pub fn arrange(sizes: &[(f64, f64)], area: Area) -> Vec<Area> {
    let n = sizes.len();
    if n == 0 {
        return Vec::new();
    }
    let sizes: Vec<(f64, f64)> = sizes
        .iter()
        .map(|&(w, h)| (w.max(1.0), h.max(1.0)))
        .collect();
    let label = LABEL_GAP + LABEL_LINE;
    let mut best: Option<(f64, Vec<std::ops::Range<usize>>)> = None;
    for rows in 1..=n {
        let split = split_rows(&sizes, rows);
        let widest = split
            .iter()
            .map(|r| {
                let w: f64 = sizes[r.clone()].iter().map(|s| s.0).sum();
                (w, r.len())
            })
            .map(|(w, k)| (area.w - (k as f64 - 1.0) * WINDOW_GAP).max(1.0) / w)
            .fold(f64::INFINITY, f64::min);
        let heights: f64 = split
            .iter()
            .map(|r| sizes[r.clone()].iter().map(|s| s.1).fold(0.0, f64::max))
            .sum();
        let room = area.h - rows as f64 * label - (rows as f64 - 1.0) * WINDOW_GAP;
        let k = widest.min(room.max(1.0) / heights).min(1.0);
        if best.as_ref().is_none_or(|(b, _)| k > *b + 1e-9) {
            best = Some((k, split));
        }
    }
    let Some((k, split)) = best else {
        return Vec::new();
    };
    let row_height =
        |r: &std::ops::Range<usize>| sizes[r.clone()].iter().map(|s| s.1).fold(0.0, f64::max) * k;
    let total: f64 = split.iter().map(|r| row_height(r) + label).sum::<f64>()
        + (split.len() as f64 - 1.0) * WINDOW_GAP;
    let mut y = area.y + ((area.h - total) / 2.0).max(0.0);
    let mut out = Vec::with_capacity(n);
    for r in &split {
        let height = row_height(r);
        let width: f64 = sizes[r.clone()].iter().map(|s| s.0 * k).sum::<f64>()
            + (r.len() as f64 - 1.0) * WINDOW_GAP;
        let mut x = area.x + (area.w - width) / 2.0;
        for &(w, h) in &sizes[r.clone()] {
            let (w, h) = (w * k, h * k);
            out.push(Area {
                x,
                y: y + (height - h) / 2.0,
                w,
                h,
            });
            x += w + WINDOW_GAP;
        }
        y += height + label + WINDOW_GAP;
    }
    out
}

/// The windows cut into `rows` rows in their order, the rows' widths as
/// even as the order allows.
fn split_rows(sizes: &[(f64, f64)], rows: usize) -> Vec<std::ops::Range<usize>> {
    let total: f64 = sizes.iter().map(|s| s.0).sum();
    let mut out = Vec::with_capacity(rows);
    let mut start = 0;
    let mut sum = 0.0;
    for row in 1..rows {
        let target = total * row as f64 / rows as f64;
        let mut end = start;
        // At least one window a row, and one left for each row after.
        while end < sizes.len() - (rows - row) {
            let next = sum + sizes[end].0;
            if end > start && (next - target).abs() > (sum - target).abs() {
                break;
            }
            sum = next;
            end += 1;
        }
        out.push(start..end);
        start = end;
    }
    out.push(start..sizes.len());
    out
}

/// The window after `from` that way (`dx`, `dy`), by their places: the
/// nearest one whose centre is on that side; none that way, `from`.
pub fn step(
    slots: &[(WindowId, Area)],
    from: Option<WindowId>,
    dx: i32,
    dy: i32,
) -> Option<WindowId> {
    let Some(from_at) = from.and_then(|f| slots.iter().find(|(id, _)| *id == f)) else {
        return slots.first().map(|(id, _)| *id);
    };
    let (fx, fy) = from_at.1.centre();
    slots
        .iter()
        .filter(|(id, _)| Some(*id) != from)
        .filter_map(|(id, a)| {
            let (x, y) = a.centre();
            let (ax, ay) = (x - fx, y - fy);
            let along = ax * f64::from(dx) + ay * f64::from(dy);
            if along <= 1.0 {
                return None;
            }
            let across = (ax * f64::from(dy)).abs() + (ay * f64::from(dx)).abs();
            Some((along + 2.0 * across, *id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
        .or(from)
}

/// What is under the pointer in the overview.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    /// A window's ✕ (it shows over the window under the pointer).
    Close(WindowId),
    Window(WindowId),
    /// A desktop's thumbnail: its screen and its desktop (from 0).
    Thumb(String, usize),
    /// The rest of a screen.
    Screen(String),
}

/// A press in the overview, until it is let go or moves into a drag.
#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    pub spot: Spot,
    pub at: (f64, f64),
    pub button: u32,
}

/// A window dragged in the overview.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    pub window: WindowId,
    pub button: u32,
    /// Where it was in the layout as it lifted, and since when it lifts.
    pub from: Area,
    pub since: Duration,
    /// The grab point's share of the window, across and down.
    pub grab: (f64, f64),
    /// The pointer.
    pub at: (f64, f64),
    /// The thumbnail it would go to, let go now: its screen and desktop;
    /// and how high it may hang then, not to hide what it says.
    pub over: Option<(String, usize)>,
    pub floor: Option<f64>,
}

impl Drag {
    /// Where it is drawn at `now`, global: lifting from its place in the
    /// layout to 42 % of it, hanging under the pointer (the mockup's), so
    /// that the thumbnail it is over still shows.
    pub fn rect(&self, now: Duration, reduced: bool) -> Area {
        let (w, h) = (self.from.w * LIFT, self.from.h * LIFT);
        let lifted = Area {
            x: self.at.0 - HANG.0 * w,
            y: (self.at.1 + HANG.1).max(self.floor.unwrap_or(f64::MIN)),
            w,
            h,
        };
        if reduced {
            return lifted;
        }
        let t = Tween::new(self.since, LIFTING, Bezier::EASE_OUT).at(now);
        // Where its place would be now, following the pointer from the
        // press.
        let from = Area {
            x: self.at.0 - self.grab.0 * self.from.w,
            y: self.at.1 - self.grab.1 * self.from.h,
            ..self.from
        };
        from.lerp(lifted, t)
    }
}

/// The overview while it shows.
#[derive(Debug, Clone)]
pub struct Overview {
    pub opened: Duration,
    /// Closing since then: the windows glide back.
    pub closing: Option<Duration>,
    /// The keyboard's selection, and whether its ring shows (once an
    /// arrow moved it).
    pub selected: Option<WindowId>,
    pub ring: bool,
    /// What the pointer is over.
    pub hover: Option<Spot>,
    pub press: Option<Press>,
    pub drag: Option<Drag>,
    /// A window let go nowhere, springing back from there to its place.
    pub back: Option<(WindowId, Area, Tween)>,
    /// The agents' windows' badges.
    pub badges: HashMap<WindowId, Badge>,
    pub reduced: bool,
}

impl Overview {
    /// How far it is open at `now`, 0 to 1: the windows' way from their
    /// places to the layout, or the fade with reduced motion.
    pub fn shown(&self, now: Duration) -> f64 {
        let (duration, curve) = if self.reduced {
            (FADE, Bezier::LINEAR)
        } else {
            (GLIDE, GLIDE_CURVE)
        };
        let open = Tween::new(self.opened, duration, curve).at(now).min(1.0);
        match self.closing {
            Some(since) => (open * (1.0 - Tween::new(since, duration, curve).at(now))).max(0.0),
            None => open,
        }
    }

    /// The duration of its way in or out.
    pub fn way(&self) -> Duration {
        if self.reduced { FADE } else { GLIDE }
    }

    pub fn open(&self) -> bool {
        self.closing.is_none()
    }
}

/// A key while the overview is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Close,
    Step(i32, i32),
    Enter,
    /// `Super+N`: desktop N (from 0) of the selection's screen.
    Show(usize),
    /// `Super+Shift+N`: the selected window to desktop N.
    Send(usize),
    /// Kept from the applications, nothing else.
    Nothing,
}

impl Key {
    /// What `sym` does, or the shortcut `action` held with it.
    pub fn of(sym: Keysym, shift: bool, action: Option<crate::Action>) -> Self {
        use crate::Action;
        match action {
            Some(Action::Overview) => return Self::Close,
            Some(Action::Desktop(n)) => return Self::Show(n),
            Some(Action::SendToDesktop(n)) => return Self::Send(n),
            _ => {}
        }
        match sym {
            Keysym::Escape => Self::Close,
            Keysym::Left => Self::Step(-1, 0),
            Keysym::Right => Self::Step(1, 0),
            Keysym::Up => Self::Step(0, -1),
            Keysym::Down => Self::Step(0, 1),
            Keysym::Tab if shift => Self::Step(-1, 0),
            Keysym::ISO_Left_Tab => Self::Step(-1, 0),
            Keysym::Tab => Self::Step(1, 0),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => Self::Enter,
            _ => Self::Nothing,
        }
    }
}

/// A screen in the overview: its place and its thumbnails' and windows'
/// areas, global.
#[derive(Debug, Clone)]
pub struct Screen {
    pub output: Output,
    pub geometry: Area,
    pub thumbs: Vec<Area>,
    pub area: Area,
}

fn area_of(r: crate::model::Rect) -> Area {
    Area {
        x: f64::from(r.x),
        y: f64::from(r.y),
        w: f64::from(r.width),
        h: f64::from(r.height),
    }
}

impl State {
    /// Whether the overview takes the pointer and the keys.
    pub fn overview_open(&self) -> bool {
        self.overview.as_ref().is_some_and(Overview::open)
    }

    /// `Super+W`, four fingers down, the bar's "+ N autres": opens it,
    /// or closes it when open.
    pub fn toggle_overview(&mut self) {
        if self.overview_open() {
            self.close_overview(None);
        } else {
            self.open_overview();
        }
    }

    /// Opens the overview on every screen.
    pub fn open_overview(&mut self) {
        if self.locked() || self.overview_open() || self.space.outputs().next().is_none() {
            return;
        }
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return;
        }
        self.end_switcher(false);
        self.close_radial(None);
        self.end_popup_grab();
        let now = self.now();
        let reduced = self.reduced_motion();
        // Opened again while it closes: from where it is.
        let opened = match self
            .overview
            .as_ref()
            .and_then(|o| o.closing.map(|c| (o, c)))
        {
            Some((o, since)) => {
                let left = o.way().saturating_sub(now.saturating_sub(since));
                now.saturating_sub(left)
            }
            None => now,
        };
        let badges = badges(&self.agents_shown());
        let selected = self
            .model
            .focused()
            .filter(|id| self.model.is_shown(*id))
            .or_else(|| self.overview_slots_all().first().map(|(id, _)| *id));
        tracing::info!(windows = self.windows.len(), "the overview opens");
        self.overview = Some(Overview {
            opened,
            closing: None,
            selected,
            ring: false,
            hover: None,
            press: None,
            drag: None,
            back: None,
            badges,
            reduced,
        });
        self.overview_pointer_off();
        self.redraw = true;
    }

    /// No client sees the pointer while the overview has it.
    fn overview_pointer_off(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let at = pointer.current_location();
        if self
            .layer_surface_under(at, crate::handlers::layer_shell::Stand::Above)
            .is_some()
        {
            return;
        }
        pointer.motion(
            self,
            None,
            &MotionEvent {
                location: at,
                serial: SERIAL_COUNTER.next_serial(),
                time: self.input_time(),
            },
        );
        pointer.frame(self);
        self.frame_cursor = None;
    }

    /// Closes the overview, `focus` brought forward: the windows glide
    /// back to their places.
    pub fn close_overview(&mut self, focus: Option<WindowId>) {
        let now = self.now();
        let Some(overview) = self.overview.as_mut().filter(|o| o.open()) else {
            return;
        };
        overview.closing = Some(now);
        overview.press = None;
        overview.drag = None;
        overview.hover = None;
        tracing::info!(focus = focus.map(|f| f.0), "the overview closes");
        if let Some(id) = focus {
            self.bring(id);
        }
        self.redraw = true;
        // What is under the pointer has it again.
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            let time = self.input_time();
            self.pointer_motion(at, time);
        }
    }

    /// Gone at once (the session locking).
    pub fn drop_overview(&mut self) {
        if self.overview.take().is_some() {
            self.redraw = true;
        }
    }

    /// The agents changed: the badges follow.
    pub fn overview_agents(&mut self, agents: &[AgentShown]) {
        if let Some(overview) = &mut self.overview {
            let new = badges(agents);
            if new != overview.badges {
                overview.badges = new;
                self.redraw = true;
            }
        }
    }

    /// A window went.
    pub fn overview_window_gone(&mut self, id: WindowId) {
        let Some(overview) = &mut self.overview else {
            return;
        };
        overview.badges.remove(&id);
        if overview.selected == Some(id) {
            overview.selected = None;
        }
        if overview.drag.as_ref().is_some_and(|d| d.window == id) {
            overview.drag = None;
        }
        if overview.back.is_some_and(|(w, _, _)| w == id) {
            overview.back = None;
        }
        let gone = |s: &Spot| matches!(s, Spot::Window(w) | Spot::Close(w) if *w == id);
        if overview.hover.as_ref().is_some_and(gone) {
            overview.hover = None;
        }
        if overview.press.as_ref().is_some_and(|p| gone(&p.spot)) {
            overview.press = None;
        }
        self.redraw = true;
    }

    /// The screens, left to right, with their thumbnails and areas.
    pub fn overview_screens(&self) -> Vec<Screen> {
        let mut outputs: Vec<Output> = self.space.outputs().cloned().collect();
        outputs.sort_by_key(|o| {
            self.space
                .output_geometry(o)
                .map_or((0, 0), |g| (g.loc.x, g.loc.y))
        });
        outputs
            .into_iter()
            .filter_map(|output| {
                let geometry = self.space.output_geometry(&output)?;
                let geometry = area_of(crate::windows::rect_of(geometry));
                let work = self.work_area(&output).map_or(geometry, area_of);
                let thumbs = thumbs(geometry, work, self.strip(&output).count());
                let area = windows_area(work, &thumbs);
                Some(Screen {
                    output,
                    geometry,
                    thumbs,
                    area,
                })
            })
            .collect()
    }

    /// The windows of `output`'s desktop `desktop` in the overview's
    /// order: left to right as they stand, minimised ones after; the one
    /// dragged out.
    pub fn overview_windows(&self, output: &Output, desktop: usize) -> Vec<WindowId> {
        let dragged = self
            .overview
            .as_ref()
            .and_then(|o| o.drag.as_ref())
            .map(|d| d.window);
        let mut ids: Vec<(bool, i32, i32, WindowId)> = self
            .windows_on(output)
            .into_iter()
            .filter(|id| Some(*id) != dragged)
            .filter(|id| self.model.desktop(*id) == desktop || self.model.is_everywhere(*id))
            .filter_map(|id| {
                let r = self.home_rect(id)?;
                Some((self.model.is_minimized(id), r.x, r.y, id))
            })
            .collect();
        ids.sort_by_key(|(min, x, y, id)| (*min, *x, *y, id.0));
        ids.into_iter().map(|(_, _, _, id)| id).collect()
    }

    /// The windows of `screen`'s desktop `desktop` laid out, global.
    pub fn overview_slots(&self, screen: &Screen, desktop: usize) -> Vec<(WindowId, Area)> {
        let ids = self.overview_windows(&screen.output, desktop);
        let sizes: Vec<(f64, f64)> = ids
            .iter()
            .map(|id| {
                self.home_rect(*id).map_or((400.0, 300.0), |r| {
                    (f64::from(r.width), f64::from(r.height))
                })
            })
            .collect();
        ids.into_iter().zip(arrange(&sizes, screen.area)).collect()
    }

    /// Every screen's shown desktop's windows, laid out.
    pub fn overview_slots_all(&self) -> Vec<(WindowId, Area)> {
        self.overview_screens()
            .iter()
            .flat_map(|s| self.overview_slots(s, self.strip(&s.output).current()))
            .collect()
    }

    /// What is under `at` (global) in the overview: a window of a shown
    /// desktop, its ✕, a thumbnail, or a screen's rest.
    pub fn overview_spot(&self, at: (f64, f64)) -> Option<Spot> {
        let screens = self.overview_screens();
        let screen = screens.iter().find(|s| s.geometry.contains(at.0, at.1))?;
        for (desktop, thumb) in screen.thumbs.iter().enumerate() {
            if thumb.contains(at.0, at.1) {
                return Some(Spot::Thumb(screen.output.name(), desktop));
            }
        }
        let slots = self.overview_slots(screen, self.strip(&screen.output).current());
        let hovered = self.overview.as_ref().and_then(|o| match &o.hover {
            Some(Spot::Window(id) | Spot::Close(id)) => Some(*id),
            _ => None,
        });
        // The ✕ of the window under the pointer first: it sticks out.
        if let Some((id, rect)) = hovered.and_then(|h| slots.iter().find(|(id, _)| *id == h))
            && close_button(*rect).contains(at.0, at.1)
        {
            return Some(Spot::Close(*id));
        }
        for (id, rect) in &slots {
            if rect.contains(at.0, at.1) {
                return Some(Spot::Window(*id));
            }
        }
        Some(Spot::Screen(screen.output.name()))
    }

    /// Whether the overview takes the pointer at `at`: open, and not over
    /// the bar (or another surface over the windows), unless a press or
    /// a drag of its own goes on.
    pub fn overview_takes(&self, at: Point<f64, Logical>) -> bool {
        let Some(overview) = self.overview.as_ref().filter(|o| o.open()) else {
            return false;
        };
        overview.press.is_some()
            || overview.drag.is_some()
            || self
                .layer_surface_under(at, crate::handlers::layer_shell::Stand::Above)
                .is_none()
    }

    /// The pointer moved in the overview.
    pub fn overview_motion(&mut self, at: Point<f64, Logical>) {
        let now = self.now();
        let spot = self.overview_spot((at.x, at.y));
        // Over a thumbnail, a dragged window hangs under its words.
        let floor = match &spot {
            Some(Spot::Thumb(output, desktop)) => self
                .overview_screens()
                .into_iter()
                .find(|s| s.output.name() == *output)
                .and_then(|s| s.thumbs.get(*desktop).copied())
                .map(|t| t.y + t.h + NUMBER_GAP + NUMBER_LINE + 4.0),
            _ => None,
        };
        let screens_slots = if self
            .overview
            .as_ref()
            .and_then(|o| o.press.as_ref())
            .is_some_and(|p| matches!(p.spot, Spot::Window(_)))
        {
            self.overview_slots_all()
        } else {
            Vec::new()
        };
        let Some(overview) = &mut self.overview else {
            return;
        };
        let mut changed = false;
        // A press on a window moved far enough: it lifts.
        if let Some(press) = overview.press.clone()
            && let Spot::Window(id) = press.spot
            && (at.x - press.at.0).hypot(at.y - press.at.1) >= DRAG_SLOP
            && let Some((_, from)) = screens_slots.iter().find(|(w, _)| *w == id)
        {
            let grab = (
                ((press.at.0 - from.x) / from.w).clamp(0.0, 1.0),
                ((press.at.1 - from.y) / from.h).clamp(0.0, 1.0),
            );
            tracing::debug!(window = id.0, "a window lifts in the overview");
            overview.drag = Some(Drag {
                window: id,
                button: press.button,
                from: *from,
                since: now,
                grab,
                at: (at.x, at.y),
                over: None,
                floor: None,
            });
            overview.press = None;
            overview.hover = None;
            overview.back = None;
            changed = true;
        }
        if let Some(drag) = &mut overview.drag {
            drag.at = (at.x, at.y);
            let over = match &spot {
                Some(Spot::Thumb(output, desktop)) => Some((output.clone(), *desktop)),
                _ => None,
            };
            drag.over = over;
            drag.floor = floor;
            self.redraw = true;
            return;
        }
        let hover = match spot {
            Some(Spot::Thumb(..) | Spot::Window(_) | Spot::Close(_)) => spot,
            _ => None,
        };
        if overview.hover != hover {
            overview.hover = hover;
            changed = true;
        }
        if changed {
            self.redraw = true;
        }
    }

    /// A press in the overview.
    pub fn overview_press(&mut self, button: u32, at: Point<f64, Logical>) {
        let spot = self.overview_spot((at.x, at.y));
        let Some(overview) = &mut self.overview else {
            return;
        };
        if overview.press.is_some() || overview.drag.is_some() {
            return;
        }
        let Some(spot) = spot else {
            return;
        };
        overview.press = Some(Press {
            spot,
            at: (at.x, at.y),
            button,
        });
    }

    /// A button let go in the overview: whether it was the overview's.
    pub fn overview_release(&mut self, button: u32, at: Point<f64, Logical>) -> bool {
        let Some(overview) = &mut self.overview else {
            return false;
        };
        if let Some(drag) = overview.drag.take_if(|d| d.button == button) {
            self.overview_drop(drag, at);
            return true;
        }
        let Some(press) = overview.press.take_if(|p| p.button == button) else {
            return false;
        };
        if button != crate::grabs::BTN_LEFT {
            return true;
        }
        match press.spot {
            Spot::Close(id) => {
                tracing::debug!(window = id.0, "the overview asks a window to close");
                self.close_window(id);
            }
            Spot::Window(id) => self.close_overview(Some(id)),
            Spot::Thumb(output, desktop) => self.overview_show(&output, desktop),
            Spot::Screen(_) => self.close_overview(None),
        }
        true
    }

    /// Desktop `desktop` of screen `output` shows in the overview, which
    /// stays open: the screen goes there.
    pub fn overview_show(&mut self, output: &str, desktop: usize) {
        let Some(output) = self.space.outputs().find(|o| o.name() == output).cloned() else {
            return;
        };
        if desktop >= self.strip(&output).count() {
            return;
        }
        tracing::debug!(
            output = output.name(),
            desktop = desktop + 1,
            "the overview shows a desktop"
        );
        self.switch_desktop(&output, desktop, None);
        if let Some(overview) = &mut self.overview {
            overview.selected = self.model.focused().filter(|id| self.model.is_shown(*id));
        }
        self.redraw = true;
    }

    /// A dragged window let go at `at`: onto a thumbnail, that desktop of
    /// that screen; onto another screen, the desktop it shows; elsewhere
    /// it springs back.
    fn overview_drop(&mut self, drag: Drag, at: Point<f64, Logical>) {
        let id = drag.window;
        let now = self.now();
        let reduced = self.reduced_motion();
        let home = self.output_of(id);
        let spot = self.overview_spot((at.x, at.y));
        let target = match &spot {
            Some(Spot::Thumb(output, desktop)) => Some((output.clone(), *desktop)),
            Some(Spot::Window(_) | Spot::Close(_) | Spot::Screen(_)) => {
                let name = match spot.as_ref() {
                    Some(Spot::Screen(name)) => Some(name.clone()),
                    _ => self.space.output_under(at).next().map(Output::name),
                };
                name.filter(|n| home.as_ref().is_some_and(|h| h.name() != *n))
                    .and_then(|n| {
                        let output = self.space.outputs().find(|o| o.name() == n)?;
                        Some((n, self.strip(output).current()))
                    })
            }
            None => None,
        };
        let moved = match (target, home) {
            (Some((output, desktop)), Some(home)) if output == home.name() => {
                if desktop != self.model.desktop(id) || self.model.is_everywhere(id) {
                    tracing::info!(
                        window = id.0,
                        desktop = desktop + 1,
                        "the overview sends a window"
                    );
                    self.send_window_to_desktop(id, desktop, false);
                    true
                } else {
                    false
                }
            }
            (Some((output, desktop)), Some(_)) => {
                let to = self.space.outputs().find(|o| o.name() == output).cloned();
                match to {
                    Some(to) => {
                        tracing::info!(
                            window = id.0,
                            output,
                            desktop = desktop + 1,
                            "the overview sends a window to a screen"
                        );
                        self.send_window_to_screen(id, &to, desktop);
                        true
                    }
                    None => false,
                }
            }
            _ => false,
        };
        if let Some(overview) = &mut self.overview {
            if moved {
                overview.back = None;
            } else {
                overview.back = Some((
                    id,
                    drag.rect(now, reduced),
                    Tween::new(now, SPRING_BACK, motion::Bezier::SPRING),
                ));
            }
            overview.hover = None;
        }
        self.redraw = true;
    }

    /// A key while the overview is open.
    pub fn overview_key(&mut self, key: Key) {
        match key {
            Key::Close => self.close_overview(None),
            Key::Nothing => {}
            Key::Enter => {
                let selected = self.overview.as_ref().and_then(|o| o.selected);
                self.close_overview(selected);
            }
            Key::Step(dx, dy) => {
                let slots = self.overview_slots_all();
                if let Some(overview) = &mut self.overview {
                    let from = overview
                        .selected
                        .filter(|s| slots.iter().any(|(id, _)| id == s));
                    overview.selected = if overview.ring {
                        step(&slots, from, dx, dy)
                    } else {
                        // The first arrow shows the ring where it is.
                        from.or_else(|| slots.first().map(|(id, _)| *id))
                    };
                    overview.ring = true;
                    self.redraw = true;
                }
            }
            Key::Show(desktop) => {
                let output = self
                    .overview
                    .as_ref()
                    .and_then(|o| o.selected)
                    .filter(|_| self.overview.as_ref().is_some_and(|o| o.ring))
                    .and_then(|id| self.output_of(id))
                    .or_else(|| self.output_under_pointer());
                if let Some(output) = output {
                    self.overview_show(&output.name(), desktop);
                }
            }
            Key::Send(desktop) => {
                let Some(id) = self.overview.as_ref().and_then(|o| o.selected) else {
                    return;
                };
                let slots = self.overview_slots_all();
                let next = step(&slots, Some(id), 1, 0)
                    .filter(|n| *n != id)
                    .or_else(|| step(&slots, Some(id), -1, 0).filter(|n| *n != id));
                self.send_window_to_desktop(id, desktop, false);
                if let Some(overview) = &mut self.overview
                    && self.model.desktop(id) == desktop
                {
                    overview.selected = next;
                }
                self.redraw = true;
            }
        }
    }

    /// The overview's motions; whether something moves now.
    pub fn tick_overview(&mut self, now: Duration) -> bool {
        let reduced = self.reduced_motion();
        let empty_desktop = self.overview.is_some() && self.overview_has_empty_desktop();
        let Some(overview) = &mut self.overview else {
            return false;
        };
        if let Some(since) = overview.closing {
            if now >= since + overview.way() {
                self.overview = None;
                self.redraw = true;
                return false;
            }
            return true;
        }
        let mut moving = now < overview.opened + overview.way();
        if let Some((_, _, tween)) = overview.back {
            if tween.done(now) {
                overview.back = None;
            } else {
                moving = true;
            }
        }
        if let Some(drag) = &overview.drag {
            moving |= now < drag.since + LIFTING;
        }
        if !reduced {
            // A waiting agent's panda waves on; the sleeping ones breathe
            // as it opens.
            moving |= overview
                .badges
                .values()
                .any(|b| b.state == AgentState::Waiting);
            moving |= empty_desktop && now < overview.opened + BREATHING;
        }
        moving
    }

    fn overview_has_empty_desktop(&self) -> bool {
        !self.overview_asleep().is_empty()
    }

    /// The windows `output`'s desktop `desktop`'s thumbnail shows, front
    /// first: those on it (or on every desktop) but minimised.
    pub fn thumb_windows(&self, output: &Output, desktop: usize) -> Vec<WindowId> {
        let mut here: Vec<WindowId> = self
            .windows_on(output)
            .into_iter()
            .filter(|id| !self.model.is_minimized(*id))
            .filter(|id| self.model.desktop(*id) == desktop || self.model.is_everywhere(*id))
            .collect();
        here.reverse();
        here
    }

    /// The thumbnails of desktops with no window, where a panda sleeps:
    /// their screen and desktop (from 0).
    pub fn overview_asleep(&self) -> Vec<(String, usize)> {
        self.space
            .outputs()
            .flat_map(|o| {
                (0..self.strip(o).count())
                    .filter(|d| self.thumb_windows(o, *d).is_empty())
                    .map(|d| (o.name(), d))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Whether the overview hides the windows of `output` as they are
    /// (they glide to the layout), or how much of them shows as it fades
    /// in with reduced motion: `None` without the overview.
    pub fn overview_cover(&self, now: Duration) -> Option<f32> {
        let overview = self.overview.as_ref()?;
        Some(if overview.reduced {
            (1.0 - overview.shown(now)) as f32
        } else {
            0.0
        })
    }
}

/// The ✕ of a window at `rect` in the layout: 24 px, over its top-right
/// corner.
pub fn close_button(rect: Area) -> Area {
    Area {
        x: rect.x + rect.w - CLOSE_BUTTON + 6.0,
        y: rect.y - 6.0,
        w: CLOSE_BUTTON,
        h: CLOSE_BUTTON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: Area, b: Area) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn windows_are_laid_out_in_order_never_larger_never_overlapping() {
        let area = Area {
            x: 100.0,
            y: 300.0,
            w: 1700.0,
            h: 700.0,
        };
        let sizes = [
            (1200.0, 800.0),
            (800.0, 600.0),
            (600.0, 900.0),
            (300.0, 200.0),
            (1800.0, 1000.0),
        ];
        let laid = arrange(&sizes, area);
        assert_eq!(laid.len(), sizes.len());
        for (i, a) in laid.iter().enumerate() {
            // Never larger, the aspect kept.
            assert!(
                a.w <= sizes[i].0 + 1e-6 && a.h <= sizes[i].1 + 1e-6,
                "{a:?}"
            );
            assert!((a.w / a.h - sizes[i].0 / sizes[i].1).abs() < 1e-6);
            // Inside the area, the title's line under it too.
            assert!(
                a.x >= area.x - 1e-6 && a.x + a.w <= area.x + area.w + 1e-6,
                "{a:?}"
            );
            assert!(a.y >= area.y - 1e-6, "{a:?}");
            assert!(
                a.y + a.h + LABEL_GAP + LABEL_LINE <= area.y + area.h + 1e-6,
                "{a:?}"
            );
            for b in &laid[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} over {b:?}");
            }
        }
        // In order: along a row left to right, rows top to bottom.
        for pair in laid.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(b.x > a.x || b.y > a.y + a.h, "{a:?} then {b:?}");
        }
    }

    #[test]
    fn a_few_small_windows_keep_their_size_on_one_row() {
        let area = Area {
            x: 0.0,
            y: 0.0,
            w: 1800.0,
            h: 800.0,
        };
        let laid = arrange(&[(400.0, 300.0), (500.0, 300.0)], area);
        assert_eq!((laid[0].w, laid[0].h), (400.0, 300.0));
        assert_eq!((laid[1].w, laid[1].h), (500.0, 300.0));
        assert_eq!(laid[0].y, laid[1].y);
        assert_eq!(laid[1].x, laid[0].x + 400.0 + WINDOW_GAP);
        // Centred.
        assert!((laid[0].x - (1800.0 - 932.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn many_windows_take_several_rows() {
        let area = Area {
            x: 0.0,
            y: 0.0,
            w: 1600.0,
            h: 900.0,
        };
        let laid = arrange(&[(1000.0, 700.0); 6], area);
        let rows: std::collections::BTreeSet<i64> =
            laid.iter().map(|a| a.y.round() as i64).collect();
        assert!(rows.len() >= 2, "{laid:?}");
    }

    #[test]
    fn the_thumbnails_are_a_ninth_of_the_screen_40_px_apart_48_from_the_top() {
        let screen = Area {
            x: 1920.0,
            y: 0.0,
            w: 2560.0,
            h: 1440.0,
        };
        let work = Area {
            x: 1928.0,
            y: 8.0,
            w: 2544.0 - 210.0,
            h: 1424.0,
        };
        let t = thumbs(screen, work, 4);
        assert_eq!(t.len(), 4);
        assert!((t[0].w - 2560.0 / 9.0).abs() < 1e-9);
        assert!((t[0].h - t[0].w * 1440.0 / 2560.0).abs() < 1e-9);
        assert_eq!(t[1].x - (t[0].x + t[0].w), THUMB_GAP);
        assert_eq!(t[0].y, 48.0);
        // Centred over the work area.
        let middle = (t[0].x + t[3].x + t[3].w) / 2.0;
        assert!((middle - (work.x + work.w / 2.0)).abs() < 1e-9);
        // Nine on a narrow screen still fit.
        let narrow = Area {
            x: 0.0,
            y: 0.0,
            w: 1280.0,
            h: 800.0,
        };
        let t = thumbs(narrow, narrow, 9);
        assert!(t[8].x + t[8].w <= 1280.0);
    }

    #[test]
    fn the_arrows_go_to_the_nearest_window_that_way() {
        let a = |x: f64, y: f64| Area {
            x,
            y,
            w: 100.0,
            h: 100.0,
        };
        let slots = vec![
            (WindowId(1), a(0.0, 0.0)),
            (WindowId(2), a(200.0, 0.0)),
            (WindowId(3), a(400.0, 10.0)),
            (WindowId(4), a(0.0, 300.0)),
            (WindowId(5), a(2400.0, 0.0)),
        ];
        assert_eq!(step(&slots, Some(WindowId(1)), 1, 0), Some(WindowId(2)));
        assert_eq!(step(&slots, Some(WindowId(2)), 1, 0), Some(WindowId(3)));
        assert_eq!(step(&slots, Some(WindowId(1)), 0, 1), Some(WindowId(4)));
        // Nothing that way: it stays.
        assert_eq!(step(&slots, Some(WindowId(1)), -1, 0), Some(WindowId(1)));
        // Onto the other screen's.
        assert_eq!(step(&slots, Some(WindowId(3)), 1, 0), Some(WindowId(5)));
        assert_eq!(step(&slots, None, 1, 0), Some(WindowId(1)));
    }

    #[test]
    fn a_windows_badge_says_its_agents_that_need_the_user_most() {
        let agent = |window: u64, state: &str| AgentShown {
            id: 0,
            state: state.into(),
            since: 0,
            source: "command".into(),
            window,
            name: "a".into(),
            note: None,
        };
        let all = [
            agent(7, "working"),
            agent(7, "waiting"),
            agent(7, "waiting"),
            agent(7, "waiting"),
            agent(9, "working"),
            agent(0, "waiting"),
        ];
        let b = badges(&all);
        assert_eq!(
            b.get(&WindowId(7)),
            Some(&Badge {
                state: AgentState::Waiting,
                count: 3
            })
        );
        assert_eq!(b[&WindowId(7)].words(true), "3 t’attendent");
        assert_eq!(b[&WindowId(7)].words(false), "3 waiting");
        assert_eq!(b[&WindowId(9)].words(true), "1 au travail");
        assert_eq!(b.len(), 2);
        let one = Badge {
            state: AgentState::Waiting,
            count: 1,
        };
        assert_eq!(one.words(true), "1 t’attend");
    }

    #[test]
    fn the_keys_of_the_overview() {
        assert_eq!(Key::of(Keysym::Escape, false, None), Key::Close);
        assert_eq!(Key::of(Keysym::Left, false, None), Key::Step(-1, 0));
        assert_eq!(Key::of(Keysym::Return, false, None), Key::Enter);
        assert_eq!(
            Key::of(Keysym::_2, false, Some(crate::Action::Desktop(1))),
            Key::Show(1)
        );
        assert_eq!(
            Key::of(Keysym::_3, true, Some(crate::Action::SendToDesktop(2))),
            Key::Send(2)
        );
        assert_eq!(
            Key::of(Keysym::w, false, Some(crate::Action::Overview)),
            Key::Close
        );
        assert_eq!(Key::of(Keysym::a, false, None), Key::Nothing);
    }

    #[test]
    fn it_glides_in_and_out_or_fades_with_reduced_motion() {
        let ms = Duration::from_millis;
        let mut o = Overview {
            opened: ms(1000),
            closing: None,
            selected: None,
            ring: false,
            hover: None,
            press: None,
            drag: None,
            back: None,
            badges: HashMap::new(),
            reduced: false,
        };
        assert_eq!(o.shown(ms(1000)), 0.0);
        assert!(o.shown(ms(1120)) > 0.5);
        assert_eq!(o.shown(ms(1240)), 1.0);
        o.closing = Some(ms(2000));
        assert_eq!(o.shown(ms(2240)), 0.0);
        o.reduced = true;
        o.closing = None;
        assert!((o.shown(ms(1060)) - 0.5).abs() < 1e-9);
        assert_eq!(o.way(), FADE);
    }
}
