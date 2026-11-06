//! A window's radial menu (#723, docs/design/windows.md §10b): its eight
//! actions around the pointer, opened by `Super` and a right click that
//! does not move over the window (a right drag still resizes, §8), or a
//! right click on its grip (§7). Each direction means something: up
//! maximises, down minimises, left and right send the window to the
//! desktop beside, up-right keeps it above the others, up-left puts it on
//! every desktop, down-right sends it to the other screen, down-left
//! closes it.
//!
//! The menu stays open once the button is up. Moving into a sector lights
//! it; crossing a sector's outer rim picks it, without a click; a click in
//! a lit sector picks it too. Close is never picked by a gesture: resting
//! on it [`CLOSE_REST`] fills an arc along its rim, then a click asks the
//! window to close. The arrows and `Tab` move the lit sector, `Enter`
//! picks (Close too), `Esc`, a click in the hub or outside, or the window
//! going closes the menu. The compositor keeps the pointer and the keys
//! while it is open: no client sees them.
//!
//! The geometry and the decisions are here, tested without a compositor;
//! `radial_draw` draws it. The actions are the same as the bar's menu of a
//! window ([`State::run_window_action`]), which the shell asks for.

use std::time::Duration;

use hocus_link::{MenuScreen, ToShell, WindowAction, WindowMenu, WindowStates};
use smithay::{
    input::keyboard::Keysym,
    output::Output,
    utils::{Logical, Point},
};

use crate::{
    State,
    model::{self, Mode, Rect, WindowId, Zone},
    motion::{self, Bezier, Tween},
    windows::rect_of,
};

/// The hub's radius, the ring's outer one, and the rim around it, logical
/// pixels (the mockup's).
pub const HUB: f64 = 44.0;
pub const RING: f64 = 116.0;
pub const RIM: f64 = 6.0;
/// Half the menu's square, its labels included: it is pushed in to stay
/// whole on its screen by this much from the pointer.
pub const REACH: f64 = 196.0;
/// A press and release this close together is a click: Super and a right
/// drag further resizes.
pub const CLICK_SLOP: f64 = 6.0;
/// Resting this long on Close arms it.
pub const CLOSE_REST: Duration = Duration::from_millis(300);
/// It opens over this (scale 0.85 → 1 from its centre and a fade; a fade
/// alone with reduced motion), and closes over [`CLOSE`].
pub const OPEN: Duration = Duration::from_millis(140);
pub const CLOSE: Duration = Duration::from_millis(100);
pub const OPEN_SCALE: f64 = 0.85;
/// A picked sector flashes this long as the menu goes.
pub const FLASH: Duration = Duration::from_millis(80);

/// A direction around the hub, clockwise from up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sector {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl Sector {
    pub const ALL: [Self; 8] = [
        Self::Up,
        Self::UpRight,
        Self::Right,
        Self::DownRight,
        Self::Down,
        Self::DownLeft,
        Self::Left,
        Self::UpLeft,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// Its middle's angle, radians, as `atan2(dy, dx)` with `y` down: up
    /// is −π/2.
    pub fn angle(self) -> f64 {
        (self.index() as f64 * 45.0 - 90.0).to_radians()
    }

    /// The sector the direction `(dx, dy)` points into.
    pub fn toward(dx: f64, dy: f64) -> Self {
        let degrees = dy.atan2(dx).to_degrees() + 90.0;
        let index = (degrees / 45.0).round().rem_euclid(8.0) as usize;
        Self::ALL[index % 8]
    }

    /// What it does.
    pub fn action(self) -> WindowAction {
        match self {
            Self::Up => WindowAction::Maximize,
            Self::UpRight => WindowAction::Above,
            Self::Right => WindowAction::NextDesktop,
            Self::DownRight => WindowAction::OtherScreen,
            Self::Down => WindowAction::Minimize,
            Self::DownLeft => WindowAction::Close,
            Self::Left => WindowAction::PreviousDesktop,
            Self::UpLeft => WindowAction::AllDesktops,
        }
    }
}

/// Where the menu's centre goes for a pointer at `pointer` on a screen
/// `screen` (global logical): on the pointer, pushed in to stay whole on
/// the screen, labels included; centred on a screen too small for it.
pub fn place(pointer: (f64, f64), screen: Rect) -> (f64, f64) {
    let keep = |p: f64, start: f64, length: f64| {
        if length < 2.0 * REACH {
            start + length / 2.0
        } else {
            p.clamp(start + REACH, start + length - REACH)
        }
    };
    (
        keep(pointer.0, f64::from(screen.x), f64::from(screen.width)),
        keep(pointer.1, f64::from(screen.y), f64::from(screen.height)),
    )
}

/// Resting on Close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rest {
    /// Not on it.
    Away,
    /// On it since then: the arc fills.
    Since(Duration),
    /// Rested long enough: a click closes the window.
    Armed,
}

/// A key while the menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Arrow(i32, i32),
    Tab {
        back: bool,
    },
    Enter,
    Escape,
    /// Any other: nothing.
    Other,
}

impl Key {
    pub fn of(sym: Keysym, shift: bool) -> Self {
        match sym {
            Keysym::Up | Keysym::KP_Up => Self::Arrow(0, -1),
            Keysym::Down | Keysym::KP_Down => Self::Arrow(0, 1),
            Keysym::Left | Keysym::KP_Left => Self::Arrow(-1, 0),
            Keysym::Right | Keysym::KP_Right => Self::Arrow(1, 0),
            Keysym::Tab => Self::Tab { back: shift },
            Keysym::ISO_Left_Tab => Self::Tab { back: true },
            Keysym::Return | Keysym::KP_Enter | Keysym::space => Self::Enter,
            Keysym::Escape => Self::Escape,
            _ => Self::Other,
        }
    }
}

/// What the menu does after a move, a press or a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    /// It goes, running that sector's action.
    Pick(Sector),
    /// It goes, running nothing.
    Close,
}

/// The menu while it is open.
#[derive(Debug, Clone)]
pub struct Radial {
    pub window: WindowId,
    pub output: Output,
    /// Its centre, global logical; and where the pointer was as it opened.
    pub centre: (f64, f64),
    pub origin: (f64, f64),
    /// Which sectors act (no desktop on that side, one screen: inert).
    pub enabled: [bool; 8],
    /// The toggles that are on (above, all desktops).
    pub on: [bool; 8],
    /// The window is maximised: up restores.
    pub maximized: bool,
    /// The window's desktop, from 0, and the other screen's name.
    pub desktop: usize,
    pub other_screen: Option<String>,
    /// Its application's id, for the hub's letter.
    pub app: String,
    /// It hosts an agent that waits: the hub shows its panda.
    pub waiting: bool,
    pub lit: Option<Sector>,
    pub rest: Rest,
    /// The pointer as last seen.
    pub last: (f64, f64),
    pub opened: Duration,
}

impl Radial {
    fn enabled(&self, sector: Sector) -> bool {
        self.enabled[sector.index()]
    }

    pub fn is_on(&self, sector: Sector) -> bool {
        self.on[sector.index()]
    }

    fn relative(&self, at: (f64, f64)) -> (f64, f64) {
        (at.0 - self.centre.0, at.1 - self.centre.1)
    }

    /// The sector a pointer at `at` lights: between the hub and the
    /// menu's edge, an enabled one.
    fn sector_at(&self, at: (f64, f64)) -> Option<Sector> {
        let (dx, dy) = self.relative(at);
        let r = dx.hypot(dy);
        if !(HUB..=REACH).contains(&r) {
            return None;
        }
        Some(Sector::toward(dx, dy)).filter(|s| self.enabled(*s))
    }

    fn light(&mut self, sector: Option<Sector>, now: Duration) {
        if sector == self.lit {
            return;
        }
        self.lit = sector;
        self.rest = if sector == Some(Sector::DownLeft) {
            Rest::Since(now)
        } else {
            Rest::Away
        };
    }

    /// Whether Close is armed at `now`.
    pub fn armed(&self, now: Duration) -> bool {
        match self.rest {
            Rest::Armed => true,
            Rest::Since(since) => now >= since + CLOSE_REST,
            Rest::Away => false,
        }
    }

    /// The arc's share along Close's rim at `now` (0 to 1), resting.
    pub fn rest_share(&self, now: Duration) -> Option<f64> {
        match self.rest {
            Rest::Away => None,
            Rest::Armed => Some(1.0),
            Rest::Since(since) => {
                let t = now.saturating_sub(since).as_secs_f64() / CLOSE_REST.as_secs_f64();
                Some(t.min(1.0))
            }
        }
    }

    /// The pointer moved to `to`: what is under it lights; crossing a
    /// sector's outer rim on the way out picks it, Close and inert ones
    /// excepted.
    pub fn moved(&mut self, to: (f64, f64), now: Duration) -> Outcome {
        let from = self.relative(self.last);
        self.last = to;
        let (dx, dy) = self.relative(to);
        let (r0, r1) = (from.0.hypot(from.1), dx.hypot(dy));
        if r0 < RING && r1 >= RING {
            // Where the way out crosses the rim: a quick flick past a
            // sector picks the one it crossed, not the one it ended over.
            let (ex, ey) = (dx - from.0, dy - from.1);
            let a = ex * ex + ey * ey;
            let b = 2.0 * (from.0 * ex + from.1 * ey);
            let c = r0 * r0 - RING * RING;
            let t = if a > 0.0 {
                ((-b + (b * b - 4.0 * a * c).max(0.0).sqrt()) / (2.0 * a)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let (cx, cy) = (from.0 + t * ex, from.1 + t * ey);
            let crossed = Sector::toward(cx, cy);
            if self.enabled(crossed) && crossed != Sector::DownLeft {
                self.light(Some(crossed), now);
                return Outcome::Pick(crossed);
            }
        }
        let under = self.sector_at(to);
        self.light(under, now);
        Outcome::Nothing
    }

    /// A press at `at`: a sector picks (Close once armed), the hub or
    /// outside closes the menu.
    pub fn pressed(&mut self, at: (f64, f64), now: Duration) -> Outcome {
        let (dx, dy) = self.relative(at);
        let r = dx.hypot(dy);
        if !(HUB..=REACH).contains(&r) {
            return Outcome::Close;
        }
        let sector = Sector::toward(dx, dy);
        if !self.enabled(sector) {
            return Outcome::Nothing;
        }
        if sector == Sector::DownLeft {
            if self.lit == Some(sector) && self.armed(now) {
                return Outcome::Pick(sector);
            }
            // A click before the rest: ignored, the rest goes on.
            self.light(Some(sector), now);
            return Outcome::Nothing;
        }
        self.light(Some(sector), now);
        Outcome::Pick(sector)
    }

    /// A key: the arrows step from the lit sector that way, `Tab` goes
    /// round, `Enter` picks the lit one (Close too, without waiting).
    pub fn key(&mut self, key: Key, now: Duration) -> Outcome {
        match key {
            Key::Arrow(x, y) => {
                // From the lit one, half a step out, then that way: from
                // up, right goes up-right.
                let (lx, ly) = self.lit.map_or((0.0, 0.0), |s| {
                    let (sin, cos) = s.angle().sin_cos();
                    (cos * 0.5, sin * 0.5)
                });
                let angle = (ly + f64::from(y)).atan2(lx + f64::from(x));
                let to = self.nearest_enabled(angle);
                self.light(to, now);
                Outcome::Nothing
            }
            Key::Tab { back } => {
                let order: Vec<Sector> = Sector::ALL
                    .into_iter()
                    .filter(|s| self.enabled(*s))
                    .collect();
                let n = order.len();
                if n == 0 {
                    return Outcome::Nothing;
                }
                let at = self.lit.and_then(|l| order.iter().position(|s| *s == l));
                let next = match (at, back) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(p), false) => (p + 1) % n,
                    (Some(p), true) => (p + n - 1) % n,
                };
                self.light(Some(order[next]), now);
                Outcome::Nothing
            }
            Key::Enter => self.lit.map_or(Outcome::Nothing, Outcome::Pick),
            Key::Escape => Outcome::Close,
            Key::Other => Outcome::Nothing,
        }
    }

    /// The enabled sector nearest the direction `angle` (radians).
    fn nearest_enabled(&self, angle: f64) -> Option<Sector> {
        let apart = |a: f64, b: f64| {
            let d = (a - b).rem_euclid(std::f64::consts::TAU);
            d.min(std::f64::consts::TAU - d)
        };
        Sector::ALL
            .into_iter()
            .filter(|s| self.enabled(*s))
            .min_by(|a, b| apart(a.angle(), angle).total_cmp(&apart(b.angle(), angle)))
    }

    /// Whether the arc still fills at `now`; Close arms once it is full.
    pub fn tick(&mut self, now: Duration) -> bool {
        if let Rest::Since(since) = self.rest {
            if now >= since + CLOSE_REST {
                self.rest = Rest::Armed;
            } else {
                return true;
            }
        }
        false
    }
}

/// The menu as tests see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    pub window: WindowId,
    pub centre: (f64, f64),
    pub lit: Option<Sector>,
    pub armed: bool,
    pub enabled: [bool; 8],
    pub on: [bool; 8],
}

/// A right press over a window that may become the menu: released within
/// [`CLICK_SLOP`], it opens; moved further with `Super`, the window
/// resizes as it always did; moved further from the grip, nothing.
#[derive(Debug, Clone, Copy)]
pub struct Press {
    pub window: WindowId,
    pub at: Point<f64, Logical>,
    pub button: u32,
    pub serial: smithay::utils::Serial,
    /// With `Super` (a drag resizes), or on the grip (a drag does nothing).
    pub resizes: bool,
}

/// The menu going, and the sector it picked, flashing.
#[derive(Debug, Clone)]
pub struct Going {
    pub radial: Radial,
    pub picked: Option<Sector>,
    pub tween: Tween,
}

impl State {
    /// Opens window `id`'s menu at the pointer's `at` (global).
    pub fn open_radial(&mut self, id: WindowId, at: Point<f64, Logical>) {
        if self.radial.is_some() || !self.windows.contains_key(&id) {
            return;
        }
        let Some(output) = self
            .space
            .output_under(at)
            .next()
            .cloned()
            .or_else(|| self.output_of(id))
        else {
            return;
        };
        let Some(screen) = self.space.output_geometry(&output).map(rect_of) else {
            return;
        };
        let centre = place((at.x, at.y), screen);
        let home = self.output_of(id).unwrap_or_else(|| output.clone());
        let strip = self.strip(&home);
        let desktop = self.model.desktop(id);
        let other = self.next_screen(&home);
        let mut enabled = [true; 8];
        enabled[Sector::Left.index()] = desktop > 0;
        enabled[Sector::Right.index()] = desktop + 1 < strip.count();
        enabled[Sector::DownRight.index()] = other.is_some();
        let mut on = [false; 8];
        on[Sector::UpRight.index()] = self.model.is_above(id);
        on[Sector::UpLeft.index()] = self.model.is_everywhere(id);
        let waiting = self
            .agents_shown()
            .iter()
            .any(|a| a.window == id.0 && a.state == "waiting");
        let app = self
            .windows
            .get(&id)
            .and_then(|m| crate::windows::app_id(&m.window))
            .unwrap_or_default();
        let now = self.now();
        tracing::debug!(window = id.0, ?centre, "the radial menu opens");
        self.end_popup_grab();
        self.radial_out = None;
        self.radial = Some(Radial {
            window: id,
            output,
            centre,
            origin: (at.x, at.y),
            enabled,
            on,
            maximized: self.model.mode(id) == Mode::Snapped(Zone::Maximized),
            desktop,
            other_screen: other.map(|o| crate::radial_draw::screen_label(&o)),
            app,
            waiting,
            lit: None,
            rest: Rest::Away,
            last: (at.x, at.y),
            opened: now,
        });
        // No client sees the pointer while it is open.
        if let Some(pointer) = self.seat.get_pointer() {
            pointer.motion(
                self,
                None,
                &smithay::input::pointer::MotionEvent {
                    location: at,
                    serial: smithay::utils::SERIAL_COUNTER.next_serial(),
                    time: self.input_time(),
                },
            );
            pointer.frame(self);
        }
        self.frame_cursor = None;
        self.redraw = true;
    }

    /// The menu goes, `picked` flashing; its action runs at once.
    pub fn close_radial(&mut self, picked: Option<Sector>) {
        let Some(radial) = self.radial.take() else {
            return;
        };
        let now = self.now();
        let duration = if picked.is_some() {
            FLASH + CLOSE
        } else {
            CLOSE
        };
        self.radial_out = Some(Going {
            radial: radial.clone(),
            picked,
            tween: Tween::new(now, duration, Bezier::EASE_IN),
        });
        self.redraw = true;
        if let Some(sector) = picked {
            tracing::info!(
                window = radial.window.0,
                action = sector.action().name(),
                "radial menu"
            );
            self.run_window_action(radial.window, sector.action());
        }
        // What is under the pointer has it again.
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            let time = self.input_time();
            self.pointer_motion(at, time);
        }
    }

    /// The pointer moved while the menu is open.
    pub(crate) fn radial_motion(&mut self, at: Point<f64, Logical>) {
        let now = self.now();
        let Some(radial) = &mut self.radial else {
            return;
        };
        let lit = radial.lit;
        match radial.moved((at.x, at.y), now) {
            Outcome::Pick(sector) => self.close_radial(Some(sector)),
            Outcome::Close => self.close_radial(None),
            Outcome::Nothing => {
                if self.radial.as_ref().is_some_and(|r| r.lit != lit) {
                    self.redraw = true;
                }
            }
        }
    }

    /// A press while the menu is open.
    pub(crate) fn radial_press(&mut self, at: Point<f64, Logical>) {
        let now = self.now();
        let Some(radial) = &mut self.radial else {
            return;
        };
        let outcome = radial.pressed((at.x, at.y), now);
        self.redraw = true;
        match outcome {
            Outcome::Pick(sector) => self.close_radial(Some(sector)),
            Outcome::Close => self.close_radial(None),
            Outcome::Nothing => {}
        }
    }

    /// A key while the menu is open.
    pub(crate) fn radial_key(&mut self, key: Key) {
        let now = self.now();
        let Some(radial) = &mut self.radial else {
            return;
        };
        let outcome = radial.key(key, now);
        self.redraw = true;
        match outcome {
            Outcome::Pick(sector) => self.close_radial(Some(sector)),
            Outcome::Close => self.close_radial(None),
            Outcome::Nothing => {}
        }
    }

    /// A window went: its menu goes.
    pub(crate) fn radial_window_gone(&mut self, id: WindowId) {
        if self.radial.as_ref().is_some_and(|r| r.window == id) {
            self.close_radial(None);
        }
        if self.radial_press.is_some_and(|p| p.window == id) {
            self.radial_press = None;
        }
    }

    /// The menu's motions; whether something moves now.
    pub fn tick_radial(&mut self, now: Duration) -> bool {
        let mut moving = false;
        if let Some(radial) = &mut self.radial {
            moving |= now < radial.opened + OPEN;
            moving |= radial.tick(now);
            // The panda bobs.
            moving |= radial.waiting && !self.reduced_motion();
        }
        if let Some(going) = &self.radial_out {
            if going.tween.done(now) {
                self.radial_out = None;
            } else {
                moving = true;
            }
        }
        moving
    }

    /// The menu as drawn at `now`: what it shows, how much (0 to 1) and
    /// at what scale; the sector flashing as it goes.
    pub(crate) fn radial_shown(
        &self,
        now: Duration,
    ) -> Option<(&Radial, f32, f64, Option<Sector>)> {
        let reduced = self.reduced_motion();
        if let Some(radial) = &self.radial {
            let t = if radial.opened + OPEN <= now {
                1.0
            } else {
                Tween::new(radial.opened, OPEN, Bezier::EASE_OUT).at(now)
            };
            let scale = if reduced {
                1.0
            } else {
                motion::lerp(OPEN_SCALE, 1.0, t)
            };
            return Some((radial, t as f32, scale, None));
        }
        let going = self.radial_out.as_ref()?;
        // The flash first, then the fade.
        let flash = going.picked.map_or(Duration::ZERO, |_| FLASH);
        let since = now.saturating_sub(going.tween.start);
        let t = if since <= flash {
            0.0
        } else {
            ((since - flash).as_secs_f64() / CLOSE.as_secs_f64()).min(1.0)
        };
        let scale = if reduced {
            1.0
        } else {
            motion::lerp(1.0, OPEN_SCALE, t)
        };
        Some((&going.radial, (1.0 - t) as f32, scale, going.picked))
    }

    /// The screen after `output` by position, left to right then top to
    /// bottom, round the end: none with one screen.
    pub(crate) fn next_screen(&self, output: &Output) -> Option<Output> {
        let mut outputs: Vec<(Output, (i32, i32))> = self
            .space
            .outputs()
            .filter_map(|o| {
                let g = self.space.output_geometry(o)?;
                Some((o.clone(), (g.loc.x, g.loc.y)))
            })
            .collect();
        if outputs.len() < 2 {
            return None;
        }
        outputs.sort_by_key(|(_, at)| *at);
        let at = outputs.iter().position(|(o, _)| o == output)?;
        Some(outputs[(at + 1) % outputs.len()].0.clone())
    }

    /// Runs `action` on window `id`, from its radial menu or the bar's.
    pub fn run_window_action(&mut self, id: WindowId, action: WindowAction) {
        if !self.windows.contains_key(&id) {
            return;
        }
        match action {
            WindowAction::Maximize => {
                self.unminimize(id);
                self.toggle_maximized(id);
            }
            WindowAction::Minimize => self.minimize(id),
            WindowAction::PreviousDesktop | WindowAction::NextDesktop => {
                let desktop = self.model.desktop(id) as i64;
                let to = if action == WindowAction::NextDesktop {
                    desktop + 1
                } else {
                    desktop - 1
                };
                if let Ok(to) = usize::try_from(to) {
                    self.send_window_to_desktop(id, to, false);
                }
            }
            WindowAction::Above => {
                let above = !self.model.is_above(id);
                self.model.set_above(id, above);
                self.redraw = true;
                self.tell_window_states(false);
            }
            WindowAction::AllDesktops => {
                let everywhere = !self.model.is_everywhere(id);
                self.set_everywhere(id, everywhere);
            }
            WindowAction::OtherScreen => {
                let Some(from) = self.output_of(id) else {
                    return;
                };
                if let Some(to) = self.next_screen(&from) {
                    let desktop = self.strip(&to).current();
                    self.send_window_to_screen(id, &to, desktop);
                }
            }
            WindowAction::Close => self.close_window(id),
        }
    }

    /// Window `id` is on every desktop of its screen, on the one it shows
    /// now; or back on that one only.
    pub fn set_everywhere(&mut self, id: WindowId, everywhere: bool) {
        if self.model.is_everywhere(id) == everywhere {
            return;
        }
        tracing::debug!(window = id.0, everywhere, "on all desktops");
        if everywhere && let Some(output) = self.output_of(id) {
            let current = self.strip(&output).current();
            if self.model.desktop(id) != current {
                self.model.set_desktop(id, current);
                self.unhide(id);
            }
        }
        self.model.set_everywhere(id, everywhere);
        self.redraw = true;
        self.tell_window_states(false);
    }

    /// Sends window `id` to desktop `desktop` of screen `to`, at the same
    /// share of its work area, snapped and maximised windows taking the
    /// same zone; it keeps the focus if it shows there.
    pub fn send_window_to_screen(&mut self, id: WindowId, to: &Output, desktop: usize) {
        let Some(from) = self.output_of(id) else {
            return;
        };
        if from == *to {
            if self.model.is_everywhere(id) {
                self.set_everywhere(id, false);
            }
            self.send_window_to_desktop(id, desktop, false);
            return;
        }
        let (Some(area), Some(to_area), Some(to_geometry), Some(rect)) = (
            self.work_area(&from),
            self.work_area(to),
            self.space.output_geometry(to).map(rect_of),
            self.home_rect(id),
        ) else {
            return;
        };
        let desktop = desktop.min(self.strip(to).count().saturating_sub(1));
        if self.model.is_everywhere(id) && desktop != self.strip(to).current() {
            self.set_everywhere(id, false);
        }
        let had_focus = self.model.focused() == Some(id);
        let placed = match self.model.mode(id) {
            // Too big there, it fits; back where it fits, its size again
            // (windows.md §12, #419).
            Mode::Floating => self.carried_fitting(id, rect, area, to_area),
            Mode::Snapped(zone) => zone.rect(to_area),
            Mode::Fullscreen => to_geometry,
        };
        let restore = self
            .model
            .placement(id)
            .restore
            .map(|r| model::carried(r, area, to_area));
        tracing::debug!(
            window = id.0,
            to = to.name(),
            desktop = desktop + 1,
            "sent to a screen"
        );
        self.relocate(id, to, desktop, placed, restore);
        if self.model.is_shown(id) {
            if had_focus {
                self.focus(Some(id));
            }
        } else if had_focus {
            let next = self
                .model
                .focused()
                .filter(|w| self.model.is_shown(*w))
                .or_else(|| self.model.top_shown());
            self.focus(next);
        }
        self.redraw = true;
    }

    /// The bar's menu of window `id` is about to open: what it shows.
    pub fn window_menu_asked(&mut self, id: u64) {
        let window = WindowId(id);
        if !self.windows.contains_key(&window) {
            return;
        }
        let home = self.output_of(window);
        let desktop = self.model.desktop(window);
        let everywhere = self.model.is_everywhere(window);
        let mut outputs: Vec<Output> = self.space.outputs().cloned().collect();
        outputs.sort_by_key(|o| {
            self.space
                .output_geometry(o)
                .map_or((0, 0), |g| (g.loc.x, g.loc.y))
        });
        let screens = outputs
            .iter()
            .filter_map(|o| {
                let g = self.space.output_geometry(o)?;
                let strip = self.strip(o);
                let here = (home.as_ref() == Some(o))
                    .then(|| if everywhere { strip.current() } else { desktop });
                Some(MenuScreen {
                    output: o.name(),
                    count: u8::try_from(strip.count()).unwrap_or(u8::MAX),
                    current: u8::try_from(strip.current()).unwrap_or(u8::MAX),
                    width: g.size.w.unsigned_abs(),
                    height: g.size.h.unsigned_abs(),
                    here: here.and_then(|d| u8::try_from(d).ok()),
                    internal: crate::panel::built_in(&o.name()),
                    name: crate::radial_draw::screen_label(o),
                })
            })
            .collect();
        self.tell_shell(ToShell::WindowMenu(WindowMenu {
            id,
            maximized: self.model.mode(window) == Mode::Snapped(Zone::Maximized),
            above: self.model.is_above(window),
            all_desktops: everywhere,
            screens,
        }));
    }

    /// The bar's menu sends window `id` to desktop `desktop` of `output`.
    pub fn send_to_asked(&mut self, id: u64, output: &str, desktop: u8) {
        let window = WindowId(id);
        let Some(to) = self.space.outputs().find(|o| o.name() == output).cloned() else {
            return;
        };
        if self.windows.contains_key(&window) {
            self.send_window_to_screen(window, &to, usize::from(desktop));
        }
    }

    /// The shell is told the windows kept above and those on all
    /// desktops, if they changed or `again`.
    pub fn tell_window_states(&mut self, again: bool) {
        let mut states = WindowStates {
            above: self.model.above().map(|w| w.0).collect(),
            all_desktops: self.model.everywhere().map(|w| w.0).collect(),
        };
        states.above.sort_unstable();
        states.all_desktops.sort_unstable();
        if !again && self.told_window_states.as_ref() == Some(&states) {
            return;
        }
        self.told_window_states = Some(states.clone());
        if self.shell_linked() {
            self.tell_shell(ToShell::WindowStates(states));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn menu() -> Radial {
        Radial {
            window: WindowId(1),
            output: Output::new(
                "test".into(),
                smithay::output::PhysicalProperties {
                    size: (0, 0).into(),
                    subpixel: smithay::output::Subpixel::Unknown,
                    make: String::new(),
                    model: String::new(),
                },
            ),
            centre: (500.0, 400.0),
            origin: (500.0, 400.0),
            enabled: [true; 8],
            on: [false; 8],
            maximized: false,
            desktop: 1,
            other_screen: None,
            app: String::new(),
            waiting: false,
            lit: None,
            rest: Rest::Away,
            last: (500.0, 400.0),
            opened: ms(0),
        }
    }

    #[test]
    fn each_direction_has_its_sector_and_its_action() {
        assert_eq!(Sector::toward(0.0, -1.0), Sector::Up);
        assert_eq!(Sector::toward(1.0, -1.0), Sector::UpRight);
        assert_eq!(Sector::toward(1.0, 0.0), Sector::Right);
        assert_eq!(Sector::toward(1.0, 1.0), Sector::DownRight);
        assert_eq!(Sector::toward(0.0, 1.0), Sector::Down);
        assert_eq!(Sector::toward(-1.0, 1.0), Sector::DownLeft);
        assert_eq!(Sector::toward(-1.0, 0.0), Sector::Left);
        assert_eq!(Sector::toward(-1.0, -1.0), Sector::UpLeft);
        // 22° off up is still up; 23° is up-right.
        let off = |deg: f64| {
            let a = (deg - 90.0).to_radians();
            Sector::toward(a.cos(), a.sin())
        };
        assert_eq!(off(22.0), Sector::Up);
        assert_eq!(off(23.0), Sector::UpRight);
        assert_eq!(Sector::Up.action(), WindowAction::Maximize);
        assert_eq!(Sector::DownLeft.action(), WindowAction::Close);
        for s in Sector::ALL {
            let (sin, cos) = s.angle().sin_cos();
            assert_eq!(Sector::toward(cos, sin), s);
        }
    }

    #[test]
    fn it_opens_on_the_pointer_pushed_in_by_its_reach() {
        let screen = Rect::new(0, 0, 1920, 1080);
        assert_eq!(place((900.0, 500.0), screen), (900.0, 500.0));
        // 20 px from the bottom right corner: whole on the screen.
        assert_eq!(
            place((1900.0, 1060.0), screen),
            (1920.0 - REACH, 1080.0 - REACH)
        );
        assert_eq!(place((5.0, 5.0), screen), (REACH, REACH));
        // A second screen to the right.
        let right = Rect::new(1920, 0, 2560, 1440);
        assert_eq!(place((1925.0, 700.0), right), (1920.0 + REACH, 700.0));
        // Too small: centred.
        assert_eq!(
            place((10.0, 10.0), Rect::new(0, 0, 300, 300)),
            (150.0, 150.0)
        );
    }

    #[test]
    fn moving_in_lights_and_crossing_the_rim_picks() {
        let mut m = menu();
        // In the hub, nothing.
        assert_eq!(m.moved((500.0, 380.0), ms(10)), Outcome::Nothing);
        assert_eq!(m.lit, None);
        // Up into the ring: it lights.
        assert_eq!(m.moved((500.0, 330.0), ms(20)), Outcome::Nothing);
        assert_eq!(m.lit, Some(Sector::Up));
        // Past the rim: picked.
        assert_eq!(m.moved((500.0, 270.0), ms(30)), Outcome::Pick(Sector::Up));
        // A flick from the hub straight past the rim picks the sector it
        // crossed.
        let mut m = menu();
        assert_eq!(
            m.moved((500.0 + 200.0, 400.0 + 10.0), ms(10)),
            Outcome::Pick(Sector::Right)
        );
        // Moving back in and out again picks again; started outside, a
        // move outside does nothing.
        let mut m = menu();
        m.last = (500.0, 400.0 - 150.0);
        assert_eq!(m.moved((500.0, 400.0 - 160.0), ms(10)), Outcome::Nothing);
        assert_eq!(m.lit, Some(Sector::Up));
    }

    #[test]
    fn close_is_never_picked_by_a_gesture_but_after_a_rest_and_a_click() {
        let mut m = menu();
        let close = (500.0 - 80.0, 400.0 + 80.0);
        let past = (500.0 - 100.0, 400.0 + 100.0);
        assert_eq!(m.moved(close, ms(0)), Outcome::Nothing);
        assert_eq!(m.lit, Some(Sector::DownLeft));
        assert_eq!(m.moved(past, ms(10)), Outcome::Nothing);
        assert_eq!(m.lit, Some(Sector::DownLeft));
        // Too soon: the click is ignored, the menu stays.
        assert_eq!(m.pressed(past, ms(200)), Outcome::Nothing);
        assert!(m.tick(ms(250)));
        assert_eq!(m.rest_share(ms(150)), Some(0.5));
        assert!(!m.tick(ms(300)));
        assert_eq!(m.rest, Rest::Armed);
        assert_eq!(m.pressed(past, ms(320)), Outcome::Pick(Sector::DownLeft));
        // Leaving it starts again.
        let mut m = menu();
        m.moved(close, ms(0));
        m.moved((500.0, 300.0), ms(100));
        m.moved(close, ms(200));
        assert!(!m.armed(ms(450)));
        assert!(m.armed(ms(500)));
    }

    #[test]
    fn inert_sectors_neither_light_nor_pick() {
        let mut m = menu();
        m.enabled[Sector::Left.index()] = false;
        assert_eq!(m.moved((420.0, 400.0), ms(0)), Outcome::Nothing);
        assert_eq!(m.lit, None);
        assert_eq!(m.moved((300.0, 400.0), ms(10)), Outcome::Nothing);
        assert_eq!(m.pressed((420.0, 400.0), ms(20)), Outcome::Nothing);
    }

    #[test]
    fn a_click_picks_a_sector_and_the_hub_or_outside_closes() {
        let mut m = menu();
        assert_eq!(
            m.pressed((500.0, 480.0), ms(0)),
            Outcome::Pick(Sector::Down)
        );
        let mut m = menu();
        assert_eq!(m.pressed((505.0, 405.0), ms(0)), Outcome::Close);
        assert_eq!(m.pressed((900.0, 400.0), ms(0)), Outcome::Close);
    }

    #[test]
    fn the_keys_go_round_and_enter_picks_close_at_once() {
        let mut m = menu();
        assert_eq!(m.key(Key::Arrow(0, -1), ms(0)), Outcome::Nothing);
        assert_eq!(m.lit, Some(Sector::Up));
        m.key(Key::Arrow(1, 0), ms(0));
        assert_eq!(m.lit, Some(Sector::UpRight));
        m.key(Key::Arrow(1, 0), ms(0));
        assert_eq!(m.lit, Some(Sector::Right));
        m.key(Key::Tab { back: false }, ms(0));
        assert_eq!(m.lit, Some(Sector::DownRight));
        m.key(Key::Tab { back: true }, ms(0));
        m.key(Key::Tab { back: true }, ms(0));
        assert_eq!(m.lit, Some(Sector::UpRight));
        m.lit = Some(Sector::DownLeft);
        assert_eq!(m.key(Key::Enter, ms(0)), Outcome::Pick(Sector::DownLeft));
        assert_eq!(m.key(Key::Escape, ms(0)), Outcome::Close);
        // Inert ones are skipped.
        let mut m = menu();
        m.enabled[Sector::Left.index()] = false;
        m.key(Key::Arrow(-1, 0), ms(0));
        assert_ne!(m.lit, Some(Sector::Left));
    }
}
