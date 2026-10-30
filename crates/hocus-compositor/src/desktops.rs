//! Virtual desktops, per screen (docs/design/windows.md §12): a screen's
//! desktops sit side by side in their order, like one strip, and switching
//! slides the strip, its windows travelling rather than fading. A swipe of
//! three fingers on a touchpad moves the strip with them, then completes
//! or springs back. Plain data and decisions on the frame clock, tested
//! without a compositor; `State` shows and hides the windows.
//!
//! The bar's tiles show each screen's desktops with a sketch of their
//! windows (docs/design/bar.md §4 group 8, 3.9): the shell is told them as
//! it connects, then after any frame of the screen where they changed, so
//! at most once a frame and never while nothing is drawn. A window dragged
//! or resized is told where it lands, once the grab ends: following it
//! would draw the tiles at every frame of every drag for a move of a pixel
//! or two in them. The strip's slide changes nothing but the current
//! desktop, told as the slide starts, so that the tiles' ring moves with
//! it.
//!
//! The windows' titles, for the list a tile opens on hover (3.9b), are told
//! on a line of their own, compared on their own: a terminal retitles at
//! every command, which must draw an open list but never the tiles. A
//! client may retitle without drawing (its window out of sight, on another
//! desktop): a change of title or of application asks for a look one frame
//! later, whatever is drawn, so that the coalescing stays at most once a
//! frame and nothing runs at rest.

use std::{collections::HashMap, time::Duration};

use hocus_link::{DeskWindow, DesktopTo, Desktops, SHARE, Titles, ToShell};
use smithay::{
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
};

use crate::{
    model::{Rect, WindowId},
    motion::{self, Bezier, Tween},
    windows::rect_of,
};

/// Desktops on each screen by default, and at most.
pub const DEFAULT_COUNT: usize = 4;
pub const MAX_COUNT: usize = 9;
/// Space between two desktops on the strip, logical pixels: what shows
/// of the background between them as they slide.
pub const STRIP_GAP: f64 = 24.0;
/// Finger travel across the touchpad that moves the strip by one desktop,
/// in libinput's units (about a millimetre's 4 or 5 units).
pub const SWIPE_DISTANCE: f64 = 360.0;
/// Past this share of a desktop, a swipe released goes on to the next.
pub const SWIPE_COMPLETES: f64 = 0.25;
/// Or when the fingers leave this fast, in desktops a second.
pub const SWIPE_FLICK: f64 = 1.2;
/// Finger travel before a swipe knows whether it goes across or down.
const SWIPE_AXIS: f64 = 12.0;
/// How far past the first or last desktop the strip follows, stiffly.
const OVERSCROLL: f64 = 0.12;

/// A swipe under way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swipe {
    /// Where the strip was when the fingers came down.
    start: f64,
    /// The fingers' travel so far.
    dx: f64,
    dy: f64,
    /// Across: it moves the strip. Down: not the desktops' (`None` until
    /// the fingers have gone far enough to say).
    across: Option<bool>,
    /// Desktops a second, from the last moves; and the last move's time
    /// (libinput's milliseconds).
    velocity: f64,
    last: Option<u32>,
    /// With reduced motion the strip does not follow; the release decides.
    reduced: bool,
}

/// How a screen's strip moves.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Motion {
    Still,
    /// From this place on the strip (in desktops, from 0) to the current
    /// desktop.
    Sliding {
        from: f64,
        tween: Tween,
    },
    /// Reduced motion: the previous desktop's windows fade out as the
    /// current one's fade in, nothing moving.
    Fading {
        from: usize,
        tween: Tween,
    },
    /// Following the fingers.
    Following(Swipe),
}

/// A screen's desktops and the one it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    count: usize,
    current: usize,
    motion: Motion,
    /// The desktops' dots show since then (§12's "how the current desktop
    /// is shown"); they fade after the motion ends.
    dots: Option<Duration>,
    /// When the motion last ended, for the dots to fade from.
    ended: Option<Duration>,
}

/// The dots fade in and out in this time, and stay this long once the
/// strip is still (the prototype's 300 ms fade, 360 ms after a switch).
pub const DOTS_FADE: Duration = Duration::from_millis(300);
pub const DOTS_STAY: Duration = Duration::from_millis(40);

impl Strip {
    pub fn new(count: usize) -> Self {
        Self {
            count: count.clamp(1, MAX_COUNT),
            current: 0,
            motion: Motion::Still,
            dots: None,
            ended: None,
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// The desktop shown, or being gone to.
    pub fn current(&self) -> usize {
        self.current
    }

    /// Where the strip is at `now`, in desktops from 0: desktop `d`'s
    /// windows are drawn `d - position` screens to the right.
    pub fn position(&self, now: Duration) -> f64 {
        let current = self.current as f64;
        match self.motion {
            Motion::Still | Motion::Fading { .. } => current,
            Motion::Sliding { from, tween } => motion::lerp(from, current, tween.at(now)),
            Motion::Following(swipe) => swipe.at(self.count),
        }
    }

    /// With reduced motion, how much of desktop `desktop` shows at `now`:
    /// the previous one fading out, the current one in.
    pub fn alpha(&self, desktop: usize, now: Duration) -> f32 {
        match self.motion {
            Motion::Fading { from, tween } => {
                let t = tween.linear(now) as f32;
                if desktop == self.current {
                    t
                } else if desktop == from {
                    1.0 - t
                } else {
                    0.0
                }
            }
            _ => f32::from(u8::from(desktop == self.current)),
        }
    }

    /// The slide under way, for windows that travel with it.
    pub fn tween(&self) -> Option<(f64, Tween)> {
        match self.motion {
            Motion::Sliding { from, tween } => Some((from, tween)),
            _ => None,
        }
    }

    /// Goes to `to` from wherever the strip is at `now`; the previous
    /// desktop, if it changed.
    pub fn switch(&mut self, to: usize, now: Duration, reduced: bool) -> Option<usize> {
        let to = to.min(self.count - 1);
        let from = self.position(now);
        let previous = self.current;
        self.dots = Some(self.dots_start(now));
        self.ended = None;
        if to == previous && from == to as f64 {
            self.motion = Motion::Still;
            self.ended = Some(now);
            return None;
        }
        self.current = to;
        self.motion = if reduced {
            Motion::Fading {
                from: previous,
                tween: Tween::new(now, motion::FADE_REDUCED, Bezier::LINEAR),
            }
        } else {
            Motion::Sliding {
                from,
                tween: Tween::new(now, motion::DESKTOP_SLIDE, Bezier::STRIP),
            }
        };
        (to != previous).then_some(previous)
    }

    /// The dots already showing keep their fade.
    fn dots_start(&self, now: Duration) -> Duration {
        let shown = self.dots_alpha(now);
        now.saturating_sub(DOTS_FADE.mul_f64(f64::from(shown)))
    }

    /// Its screen went: it stops on the desktop it was going to, which it
    /// shows if the screen comes back.
    pub fn still(&mut self) {
        self.motion = Motion::Still;
        self.dots = None;
        self.ended = None;
    }

    /// The fingers came down: the strip follows them from where it is.
    pub fn swipe_begin(&mut self, now: Duration, reduced: bool) {
        let start = self.position(now);
        self.motion = Motion::Following(Swipe {
            start,
            dx: 0.0,
            dy: 0.0,
            across: None,
            velocity: 0.0,
            last: None,
            reduced,
        });
        self.ended = None;
    }

    /// The fingers moved by (`dx`, `dy`) at `time` (libinput's
    /// milliseconds). Whether the strip moved.
    pub fn swipe_update(&mut self, dx: f64, dy: f64, time: u32, now: Duration) -> bool {
        let Motion::Following(swipe) = &mut self.motion else {
            return false;
        };
        swipe.dx += dx;
        swipe.dy += dy;
        if swipe.across.is_none() && swipe.dx.hypot(swipe.dy) >= SWIPE_AXIS {
            swipe.across = Some(swipe.dx.abs() >= swipe.dy.abs());
        }
        if swipe.across != Some(true) {
            return false;
        }
        // Fingers going left bring the desktop on the right.
        let step = -dx / SWIPE_DISTANCE;
        if let Some(last) = swipe.last {
            let dt = f64::from(time.wrapping_sub(last)) / 1000.0;
            if dt > 0.0 && dt < 0.25 {
                let v = step / dt;
                swipe.velocity = 0.6 * v + 0.4 * swipe.velocity;
            }
        }
        swipe.last = Some(time);
        let follows = !swipe.reduced;
        if self.dots.is_none() {
            self.dots = Some(self.dots_start(now));
        }
        follows
    }

    /// The fingers left: the strip goes on to the desktop it reached (or
    /// the next one, on a flick) or springs back. The previous desktop, if
    /// it changed.
    pub fn swipe_end(&mut self, cancelled: bool, now: Duration) -> Option<usize> {
        let Motion::Following(swipe) = self.motion else {
            return None;
        };
        let at = swipe.at(self.count);
        let target = if cancelled || swipe.across != Some(true) {
            self.current
        } else {
            swipe.target(self.current, self.count)
        };
        let previous = self.current;
        self.current = target;
        self.motion = if swipe.reduced {
            if target == previous {
                Motion::Still
            } else {
                Motion::Fading {
                    from: previous,
                    tween: Tween::new(now, motion::FADE_REDUCED, Bezier::LINEAR),
                }
            }
        } else if (at - target as f64).abs() < 1e-6 {
            Motion::Still
        } else {
            Motion::Sliding {
                from: at,
                tween: Tween::new(now, motion::DESKTOP_SLIDE, Bezier::STRIP),
            }
        };
        if self.motion == Motion::Still {
            self.ended = Some(now);
        }
        (target != previous).then_some(previous)
    }

    /// Whether the fingers are on the touchpad, moving the strip.
    pub fn swiping(&self) -> bool {
        matches!(self.motion, Motion::Following(_))
    }

    /// The motion moves on at `now`; whether something still changes on
    /// screen (the strip, or the dots).
    pub fn tick(&mut self, now: Duration) -> bool {
        let done = match self.motion {
            Motion::Sliding { tween, .. } | Motion::Fading { tween, .. } => tween.done(now),
            Motion::Still | Motion::Following(_) => false,
        };
        if done {
            self.motion = Motion::Still;
            self.ended = Some(now);
        }
        let moving = !matches!(self.motion, Motion::Still | Motion::Following(_));
        if let (Some(_), Some(ended)) = (self.dots, self.ended)
            && now >= ended + DOTS_STAY + DOTS_FADE
        {
            self.dots = None;
            self.ended = None;
        }
        let dots_moving = self.dots.is_some() && {
            let a = self.dots_alpha(now);
            // Fading in, or out.
            a > 0.0 && (a < 1.0 || self.ended.is_some())
        };
        moving || dots_moving
    }

    /// How much the dots show at `now`, 0 to 1.
    pub fn dots_alpha(&self, now: Duration) -> f32 {
        let Some(since) = self.dots else {
            return 0.0;
        };
        let fade_in = Tween::new(since, DOTS_FADE, Bezier::EASE_OUT).at(now);
        let fade_out = self.ended.map_or(0.0, |ended| {
            Tween::new(ended + DOTS_STAY, DOTS_FADE, Bezier::EASE_IN).at(now)
        });
        (fade_in * (1.0 - fade_out)).clamp(0.0, 1.0) as f32
    }
}

impl Swipe {
    /// Where the strip is with the fingers: stiff past the ends.
    fn at(&self, count: usize) -> f64 {
        if self.reduced || self.across != Some(true) {
            return self.start;
        }
        let raw = self.start - self.dx / SWIPE_DISTANCE;
        let last = (count - 1) as f64;
        let past = |over: f64| OVERSCROLL * (1.0 - 1.0 / (1.0 + over / OVERSCROLL / 3.0));
        if raw < 0.0 {
            -past(-raw)
        } else if raw > last {
            last + past(raw - last)
        } else {
            raw
        }
    }

    /// The desktop a release lands on: the one the strip is nearest, past
    /// a quarter of the way; on a flick, the next one that way.
    fn target(&self, current: usize, count: usize) -> usize {
        let travel = (self.start - self.dx / SWIPE_DISTANCE) - current as f64;
        let mut steps = (travel.abs() + (1.0 - SWIPE_COMPLETES)).floor();
        let mut sign = travel.signum();
        if steps == 0.0 && self.velocity.abs() >= SWIPE_FLICK {
            steps = 1.0;
            sign = self.velocity.signum();
        }
        let target = current as f64 + sign * steps;
        target.clamp(0.0, (count - 1) as f64).round() as usize
    }
}

/// `rect`'s place and size on `screen` in [`SHARE`]s of it, cut to the
/// screen; `None` when none of it is on it. Its edges are rounded, not its
/// size, so that windows side by side stay side by side in the tile.
pub fn share(rect: Rect, screen: Rect) -> Option<[u16; 4]> {
    if screen.width <= 0 || screen.height <= 0 {
        return None;
    }
    let along = |at: i32, from: i32, length: i32| {
        let share = f64::from(at - from) * f64::from(SHARE) / f64::from(length);
        share.round().clamp(0.0, f64::from(SHARE)) as u16
    };
    let (left, right) = (
        along(rect.x, screen.x, screen.width),
        along(rect.x + rect.width, screen.x, screen.width),
    );
    let (top, bottom) = (
        along(rect.y, screen.y, screen.height),
        along(rect.y + rect.height, screen.y, screen.height),
    );
    let inside = rect.x < screen.x + screen.width
        && rect.x + rect.width > screen.x
        && rect.y < screen.y + screen.height
        && rect.y + rect.height > screen.y;
    if !inside {
        return None;
    }
    // A sliver still shows as one.
    let left = left.min(SHARE - 1);
    let top = top.min(SHARE - 1);
    Some([
        left,
        top,
        (right.max(left + 1)) - left,
        (bottom.max(top + 1)) - top,
    ])
}

/// A look at the desktops after a change of title or application id: one
/// frame later.
const SOON: Duration = Duration::from_millis(16);

/// What each bar's tiles and list were last told, by screen.
#[derive(Debug, Default)]
pub struct Told {
    desktops: HashMap<String, Desktops>,
    titles: HashMap<String, Titles>,
    /// The look asked after a change of title, until it runs.
    soon: Option<smithay::reexports::calloop::RegistrationToken>,
}

impl Told {
    /// The screen's desktops are `now`: what to tell, if they changed since
    /// they were told. Nothing while a window is dragged or resized
    /// (`held`): where it lands is told once the grab ends.
    pub fn offer(&mut self, now: Desktops, held: bool) -> Option<Desktops> {
        if held || self.desktops.get(&now.output) == Some(&now) {
            return None;
        }
        self.desktops.insert(now.output.clone(), now.clone());
        Some(now)
    }

    /// The screen's windows' titles are `now`: what to tell, if they
    /// changed since they were told.
    pub fn offer_titles(&mut self, now: Titles) -> Option<Titles> {
        if self.titles.get(&now.output) == Some(&now) {
            return None;
        }
        self.titles.insert(now.output.clone(), now.clone());
        Some(now)
    }

    /// Nothing told: a shell that connects is told everything.
    pub fn clear(&mut self) {
        self.desktops.clear();
        self.titles.clear();
    }

    /// The screen has no bar any more.
    pub fn forget(&mut self, output: &str) {
        self.desktops.remove(output);
        self.titles.remove(output);
    }
}

impl crate::State {
    /// `output`'s desktops as its bar's tiles show them: the windows on it,
    /// minimised ones aside, bottom first.
    pub fn desktops_view(&self, output: &Output) -> Option<Desktops> {
        let screen = rect_of(self.space.output_geometry(output)?);
        let strip = self.strip(output);
        let current = strip.current();
        let focused = self.model.focused();
        let windows = self
            .windows_on(output)
            .into_iter()
            .filter(|id| !self.model.is_minimized(*id))
            .filter_map(|id| {
                let desktop = self.model.desktop(id);
                Some(DeskWindow {
                    desktop: u8::try_from(desktop).ok()?,
                    rect: share(self.home_rect(id)?, screen)?,
                    focused: focused == Some(id) && desktop == current,
                    id: id.0,
                    app: self
                        .windows
                        .get(&id)
                        .and_then(|m| crate::windows::app_id(&m.window))
                        .unwrap_or_default(),
                })
            })
            .collect();
        Some(Desktops {
            output: output.name(),
            count: u8::try_from(strip.count()).unwrap_or(u8::MAX),
            current: u8::try_from(current).unwrap_or(u8::MAX),
            windows,
        })
    }

    /// `output`'s windows' titles, in [`Self::desktops_view`]'s order.
    pub fn titles_view(&self, output: &Output) -> Titles {
        let titles = self
            .windows_on(output)
            .into_iter()
            .filter(|id| !self.model.is_minimized(*id))
            .map(|id| (id.0, crate::windows::title_of(self, id)))
            .collect();
        Titles {
            output: output.name(),
            titles,
        }
    }

    /// A frame of `output` was drawn: its bar's tiles are told its desktops
    /// if they changed, and its list its titles (see the module's
    /// documentation).
    pub fn desktops_drawn(&mut self, output: &Output) {
        if !self.shell_linked() {
            return;
        }
        if !self.panel.bars.contains_key(&output.name()) {
            self.shell.desktops.forget(&output.name());
            return;
        }
        let held = self.moving.is_some()
            || self
                .windows
                .values()
                .any(|m| m.resizing.is_some_and(|r| !r.ended));
        if let Some(view) = self.desktops_view(output)
            && let Some(told) = self.shell.desktops.offer(view, held)
        {
            self.tell_shell(ToShell::Desktops(told));
        }
        let titles = self.titles_view(output);
        if let Some(told) = self.shell.desktops.offer_titles(titles) {
            self.tell_shell(ToShell::Titles(told));
        }
    }

    /// The bars' screens, as the shell knows them.
    fn bars_outputs(&self) -> Vec<Output> {
        self.space
            .outputs()
            .filter(|o| self.panel.bars.contains_key(&o.name()))
            .cloned()
            .collect()
    }

    /// The shell connected: every bar's tiles are told their desktops, and
    /// their list the titles.
    pub fn tell_desktops(&mut self) {
        self.shell.desktops.clear();
        for output in self.bars_outputs() {
            self.desktops_drawn(&output);
        }
    }

    /// A window's title or application changed: the bars are told one
    /// frame later, if that changed what they show, without drawing a frame
    /// (a client may retitle without drawing). Changes in that frame go
    /// together.
    pub fn desktops_soon(&mut self) {
        if !self.shell_linked() || self.shell.desktops.soon.is_some() {
            return;
        }
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(SOON), |_, _, app| {
                let state = &mut app.state;
                state.shell.desktops.soon = None;
                for output in state.bars_outputs() {
                    state.desktops_drawn(&output);
                }
                TimeoutAction::Drop
            });
        match inserted {
            Ok(token) => self.shell.desktops.soon = Some(token),
            Err(err) => tracing::warn!(%err.error, "could not look at the titles again"),
        }
    }

    /// The shell asks for window `id` (a row of a desktop's list): its
    /// desktop, sliding, and the focus on it.
    pub fn window_asked(&mut self, id: u64) {
        let id = WindowId(id);
        if self.windows.contains_key(&id) {
            self.bring(id);
        }
    }

    /// The shell asks for a desktop of `output` (a tile, the wheel): as the
    /// shortcuts go there, sliding, and no further than the ends.
    pub fn desktop_asked(&mut self, output: &str, to: DesktopTo) {
        let Some(output) = self.space.outputs().find(|o| o.name() == output).cloned() else {
            return;
        };
        match to {
            DesktopTo::At(index) => {
                let index = usize::from(index);
                if index < self.strip(&output).count() {
                    self.switch_desktop(&output, index, None);
                }
            }
            DesktopTo::Next => self.switch_desktop_on(&output, 1),
            DesktopTo::Previous => self.switch_desktop_on(&output, -1),
        }
    }

    /// `output`'s desktops, as they are or new.
    pub fn strip(&self, output: &Output) -> Strip {
        self.strips
            .get(&output.name())
            .copied()
            .unwrap_or_else(|| Strip::new(self.config.desktops.count))
    }

    fn set_strip(&mut self, output: &Output, strip: Strip) {
        self.strips.insert(output.name(), strip);
        self.redraw = true;
    }

    /// The windows on `output`, on any desktop, minimised included.
    pub fn windows_on(&self, output: &Output) -> Vec<WindowId> {
        self.model
            .stack()
            .iter()
            .copied()
            .filter(|id| self.output_of(*id).as_ref() == Some(output))
            .collect()
    }

    /// Whether `id` is on the desktop its screen shows.
    pub fn on_current_desktop(&self, id: WindowId) -> bool {
        self.output_of(id)
            .is_some_and(|o| self.strip(&o).current() == self.model.desktop(id))
    }

    /// Where `id` is drawn on its screen's strip at `now`: how far right of
    /// where it is, logical pixels (a screen and the gap between desktops
    /// per desktop away from the strip's place); and
    /// how much of it shows (reduced motion fades instead). `None` when it
    /// is out of sight.
    pub fn strip_offset(&self, id: WindowId, now: Duration) -> Option<(f64, f32)> {
        let output = self.output_of(id)?;
        let strip = self.strip(&output);
        let desktop = self.model.desktop(id);
        let travel = self.windows.get(&id)?.travel;
        if self.reduced_motion() {
            let alpha = match travel {
                // Carried along: it stays.
                Some((from, to, _)) if from == to => 1.0,
                // Sent away: it fades.
                Some((_, _, tween)) => 1.0 - tween.linear(now) as f32,
                None => strip.alpha(desktop, now),
            };
            return (alpha > 0.0).then_some((0.0, alpha));
        }
        let at = match travel {
            Some((from, to, tween)) => motion::lerp(from, to, tween.at(now)),
            None => desktop as f64,
        };
        let offset = at - strip.position(now);
        (offset.abs() < 1.0).then_some((offset * self.strip_stride(&output), 1.0))
    }

    /// One desktop's width on `output`'s strip, logical pixels.
    pub fn strip_stride(&self, output: &Output) -> f64 {
        self.space
            .output_geometry(output)
            .map_or(0.0, |g| f64::from(g.size.w))
            + STRIP_GAP
    }

    /// `id` leaves the screen: its desktop is out of sight.
    fn hide(&mut self, id: WindowId) {
        let rect = self.rect_of(id);
        self.end_popup_grab_of(id);
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        if rect.is_some() {
            managed.away = rect;
        }
        managed.settle = None;
        let window = managed.window.clone();
        self.space.unmap_elem(&window);
        if self.wobble.as_ref().is_some_and(|(w, _)| *w == id) {
            self.wobble = None;
        }
        self.model.set_hidden(id, true);
    }

    /// `id`'s desktop comes in sight: it shows where it was, unless
    /// minimised.
    fn unhide(&mut self, id: WindowId) {
        self.model.set_hidden(id, false);
        if self.model.is_minimized(id) {
            return;
        }
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        let Some(rect) = managed.away.take() else {
            return;
        };
        let window = managed.window.clone();
        self.space.map_element(window, (rect.x, rect.y), false);
    }

    /// `output` shows its desktop `to` (from 0), sliding; `carried` goes
    /// with it, staying where it is on the screen.
    pub fn switch_desktop(&mut self, output: &Output, to: usize, carried: Option<WindowId>) {
        let now = self.now();
        let reduced = self.reduced_motion();
        let mut strip = self.strip(output);
        let to = to.min(strip.count() - 1);
        let changed = strip.switch(to, now, reduced);
        self.set_strip(output, strip);
        if changed.is_some() {
            self.apply_desktop(output, to, carried);
        }
    }

    /// The windows of `output`'s desktop `to` show, the others' hide, and
    /// the focus goes to the one on top.
    fn apply_desktop(&mut self, output: &Output, to: usize, carried: Option<WindowId>) {
        let strip = self.strip(output);
        tracing::debug!(output = output.name(), desktop = to + 1, "desktop");
        self.end_popup_grab();
        // A window being dragged goes along.
        let carried = carried.or(self.moving);
        for id in self.windows_on(output) {
            if Some(id) == carried {
                self.model.set_desktop(id, to);
                let now = self.now();
                if let Some(managed) = self.windows.get_mut(&id) {
                    // Drawn where the strip is as it slides: it stays put.
                    managed.travel = match strip.tween() {
                        Some((from, tween)) => Some((from, to as f64, tween)),
                        None => Some((
                            to as f64,
                            to as f64,
                            Tween::new(now, motion::FADE_REDUCED, Bezier::LINEAR),
                        )),
                    };
                }
                continue;
            }
            let shown = !self.model.placement(id).hidden;
            if self.model.desktop(id) == to {
                self.unhide(id);
            } else if shown {
                self.hide(id);
            }
        }
        // The focus goes to the desktop's window on top.
        let top =
            carried.filter(|id| self.model.is_shown(*id)).or_else(|| {
                self.model.stack().iter().rev().copied().find(|id| {
                    self.model.is_shown(*id) && self.output_of(*id).as_ref() == Some(output)
                })
            });
        let focus = top.or_else(|| self.model.focused().filter(|id| self.model.is_shown(*id)));
        self.focus(focus);
    }

    /// The desktop on that side of the current one, on the screen under
    /// the pointer.
    pub fn switch_desktop_by(&mut self, delta: i32) {
        if let Some(output) = self.output_under_pointer() {
            self.switch_desktop_on(&output, delta);
        }
    }

    /// The desktop on that side of `output`'s current one.
    fn switch_desktop_on(&mut self, output: &Output, delta: i32) {
        let strip = self.strip(output);
        let to = strip.current() as i64 + i64::from(delta);
        if (0..strip.count() as i64).contains(&to) {
            self.switch_desktop(output, to as usize, None);
        } else {
            // At the end of the strip: the dots say where it is.
            let mut strip = strip;
            strip.switch(strip.current(), self.now(), self.reduced_motion());
            self.set_strip(output, strip);
        }
    }

    /// Desktop `to` (from 0) of the screen under the pointer.
    pub fn switch_desktop_to(&mut self, to: usize) {
        if let Some(output) = self.output_under_pointer()
            && to < self.strip(&output).count()
        {
            self.switch_desktop(&output, to, None);
        }
    }

    /// Sends the focused window to desktop `to` (from 0) of its screen;
    /// `follow`: the screen goes there too, the window with it (§12).
    pub fn send_to_desktop(&mut self, to: usize, follow: bool) {
        let Some(id) = self.model.focused() else {
            return;
        };
        let Some(output) = self.output_of(id) else {
            return;
        };
        let strip = self.strip(&output);
        if to >= strip.count() {
            return;
        }
        let from = self.model.desktop(id);
        if follow {
            self.model.set_desktop(id, to);
            self.switch_desktop(&output, to, Some(id));
            return;
        }
        if from == to {
            return;
        }
        tracing::debug!(window = id.0, desktop = to + 1, "sent");
        let now = self.now();
        self.model.set_desktop(id, to);
        self.hide(id);
        let reduced = self.reduced_motion();
        if let Some(managed) = self.windows.get_mut(&id) {
            let duration = if reduced {
                motion::FADE_REDUCED
            } else {
                motion::DESKTOP_SLIDE
            };
            // Out of the screen toward its desktop's side: a screen's
            // width, however far its desktop is, easing in as windows
            // leave (closing, minimising), so that it is seen going.
            let toward = if to > from { 1.0 } else { -1.0 };
            managed.travel = Some((
                from as f64,
                from as f64 + toward,
                Tween::new(now, duration, Bezier::EASE_IN),
            ));
        }
        // The dots show where it went.
        let mut strip = strip;
        strip.switch(strip.current(), now, reduced);
        self.set_strip(&output, strip);
        let next = self
            .model
            .stack()
            .iter()
            .rev()
            .copied()
            .find(|w| self.model.is_shown(*w) && self.output_of(*w).as_ref() == Some(&output))
            .or_else(|| self.model.focused());
        self.focus(next);
    }

    /// Takes the focused window to the desktop on that side of its
    /// screen's current one, going there with it; past the first or the
    /// last, it stays, and the dots say where the strip ends.
    pub fn take_to_desktop_by(&mut self, delta: i32) {
        let Some(id) = self.model.focused() else {
            return;
        };
        let Some(output) = self.output_of(id) else {
            return;
        };
        let strip = self.strip(&output);
        let to = strip.current() as i64 + i64::from(delta);
        if (0..strip.count() as i64).contains(&to) {
            self.send_to_desktop(to as usize, true);
        } else {
            self.switch_desktop_on(&output, delta);
        }
    }

    /// Brings `id` forward: to its desktop, back from minimised, focused.
    pub fn bring(&mut self, id: WindowId) {
        if let Some(output) = self.output_of(id) {
            let desktop = self.model.desktop(id);
            if self.strip(&output).current() != desktop {
                self.switch_desktop(&output, desktop, None);
            }
        }
        self.unminimize(id);
        if self.model.is_shown(id) {
            self.focus(Some(id));
        }
    }

    /// Three fingers came down on the touchpad: the strip under the
    /// pointer follows them.
    pub fn desktop_swipe_begin(&mut self) {
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let mut strip = self.strip(&output);
        strip.swipe_begin(self.now(), self.reduced_motion());
        self.swiping = Some(output.clone());
        self.set_strip(&output, strip);
    }

    pub fn desktop_swipe_update(&mut self, dx: f64, dy: f64, time: u32) {
        let Some(output) = self.swiping.clone() else {
            return;
        };
        let mut strip = self.strip(&output);
        let now = self.now();
        // The desktops coming in show as they come: their windows are
        // drawn from where they are, out of sight or not.
        let moved = strip.swipe_update(dx, dy, time, now);
        self.strips.insert(output.name(), strip);
        if moved || strip.dots_alpha(now) < 1.0 {
            self.redraw = true;
        }
    }

    pub fn desktop_swipe_end(&mut self, cancelled: bool) {
        let Some(output) = self.swiping.take() else {
            return;
        };
        let mut strip = self.strip(&output);
        let changed = strip.swipe_end(cancelled, self.now());
        self.set_strip(&output, strip);
        // The strip goes on from where the fingers left it; the windows
        // change desktops as with a switch.
        if changed.is_some() {
            self.apply_desktop(&output, strip.current(), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn switching_slides_the_strip_in_320_ms_past_the_desktops_between() {
        let mut strip = Strip::new(4);
        assert_eq!(strip.switch(2, ms(1000), false), Some(0));
        assert_eq!(strip.current(), 2);
        assert_eq!(strip.position(ms(1000)), 0.0);
        // Halfway in time, past desktop 2 (index 1) already: the curve
        // starts fast.
        let mid = strip.position(ms(1160));
        assert!(mid > 1.0 && mid < 2.0, "{mid}");
        assert!(strip.tick(ms(1160)));
        assert_eq!(strip.position(ms(1320)), 2.0);
        // Still once it lands; the dots fade a moment later.
        assert!(strip.tick(ms(1320)));
        assert!(strip.dots_alpha(ms(1320)) > 0.9);
        assert!(!strip.tick(ms(1320) + DOTS_STAY + DOTS_FADE));
        assert_eq!(strip.dots_alpha(ms(2000)), 0.0);
        // The same desktop again: nothing moves.
        assert_eq!(strip.switch(2, ms(3000), false), None);
        assert_eq!(strip.position(ms(3000)), 2.0);
    }

    #[test]
    fn a_switch_during_a_slide_goes_on_from_where_the_strip_is() {
        let mut strip = Strip::new(4);
        strip.switch(3, ms(0), false);
        let there = strip.position(ms(100));
        strip.switch(0, ms(100), false);
        assert_eq!(strip.position(ms(100)), there);
        assert_eq!(strip.position(ms(420)), 0.0);
    }

    #[test]
    fn reduced_motion_fades_the_desktops_instead() {
        let mut strip = Strip::new(4);
        strip.switch(1, ms(0), true);
        assert_eq!(strip.position(ms(0)), 1.0);
        assert_eq!(strip.alpha(1, ms(60)), 0.5);
        assert_eq!(strip.alpha(0, ms(60)), 0.5);
        assert_eq!(strip.alpha(2, ms(60)), 0.0);
        strip.tick(ms(120));
        assert_eq!(strip.alpha(0, ms(120)), 0.0);
        assert_eq!(strip.alpha(1, ms(120)), 1.0);
    }

    #[test]
    fn a_swipe_follows_the_fingers_then_completes_past_a_quarter() {
        let mut strip = Strip::new(4);
        strip.swipe_begin(ms(0), false);
        // A third of a desktop to the left, slowly.
        for i in 0..10 {
            strip.swipe_update(-SWIPE_DISTANCE / 30.0, 0.0, 100 * i, ms(0));
        }
        let at = strip.position(ms(0));
        assert!((at - 1.0 / 3.0).abs() < 0.05, "{at}");
        assert_eq!(strip.swipe_end(false, ms(1000)), Some(0));
        assert_eq!(strip.current(), 1);
        assert_eq!(strip.position(ms(1000)), at);
        assert_eq!(strip.position(ms(1320)), 1.0);
    }

    #[test]
    fn a_short_slow_swipe_springs_back_and_a_flick_goes_on() {
        let mut strip = Strip::new(4);
        strip.switch(1, ms(0), false);
        strip.tick(ms(400));
        strip.swipe_begin(ms(1000), false);
        for i in 0..4 {
            strip.swipe_update(SWIPE_DISTANCE / 40.0, 0.0, 200 * i, ms(1000));
        }
        assert_eq!(strip.swipe_end(false, ms(1000)), None);
        assert_eq!(strip.current(), 1);
        assert_eq!(strip.position(ms(1320)), 1.0);
        // The same distance, fast: the desktop on the left.
        strip.swipe_begin(ms(2000), false);
        for i in 0..4 {
            strip.swipe_update(SWIPE_DISTANCE / 40.0, 0.0, 5000 + 10 * i, ms(2000));
        }
        assert_eq!(strip.swipe_end(false, ms(2000)), Some(1));
        assert_eq!(strip.current(), 0);
    }

    #[test]
    fn a_swipe_down_or_cancelled_or_past_the_ends_stays() {
        let mut strip = Strip::new(4);
        strip.swipe_begin(ms(0), false);
        strip.swipe_update(5.0, 200.0, 10, ms(0));
        strip.swipe_update(-400.0, 0.0, 20, ms(0));
        assert_eq!(strip.position(ms(0)), 0.0, "a vertical swipe");
        assert_eq!(strip.swipe_end(false, ms(0)), None);
        // Cancelled.
        strip.swipe_begin(ms(0), false);
        strip.swipe_update(-500.0, 0.0, 10, ms(0));
        assert_eq!(strip.swipe_end(true, ms(0)), None);
        // Past the first desktop: the strip barely follows, and comes back.
        strip.tick(ms(1000));
        strip.swipe_begin(ms(1000), false);
        strip.swipe_update(2000.0, 0.0, 10, ms(1000));
        let at = strip.position(ms(1000));
        assert!(at < 0.0 && at > -OVERSCROLL, "{at}");
        assert_eq!(strip.swipe_end(false, ms(1000)), None);
        assert_eq!(strip.position(ms(1320)), 0.0);
        // A long swipe can go more than one desktop.
        strip.tick(ms(2000));
        strip.swipe_begin(ms(2000), false);
        for i in 0..20 {
            strip.swipe_update(-SWIPE_DISTANCE / 8.0, 0.0, 3000 + 100 * i, ms(2000));
        }
        assert_eq!(strip.swipe_end(false, ms(2000)), Some(0));
        assert_eq!(strip.current(), 3);
    }

    #[test]
    fn a_tiles_sketch_is_the_windows_share_of_its_screen() {
        // The second screen, 2560 × 1440 right of a 1920 px one.
        let screen = Rect::new(1920, 0, 2560, 1440);
        // A left half: its right edge where a right half's left edge is.
        let left = share(Rect::new(1928, 8, 1280 - 8, 1424), screen);
        let right = share(Rect::new(3200, 8, 1272, 1424), screen);
        assert_eq!(left, Some([3, 6, 497, 988]));
        let [x, ..] = right.expect("on the screen");
        assert_eq!(x, 500);
        // Fullscreen: the whole tile.
        assert_eq!(share(screen, screen), Some([0, 0, SHARE, SHARE]));
        // Half off the screen: what is on it; off it: nothing.
        assert_eq!(
            share(Rect::new(1920 - 640, 720, 1280, 720), screen),
            Some([0, 500, 250, 500])
        );
        assert_eq!(share(Rect::new(0, 0, 1920, 1080), screen), None);
        // A sliver still shows.
        assert_eq!(
            share(Rect::new(4479, 0, 1, 1), screen),
            Some([999, 0, 1, 1])
        );
    }

    #[test]
    fn the_tiles_are_told_what_changed_and_where_a_drag_lands() {
        let view = |current: u8, x: u16| Desktops {
            output: "eDP-1".into(),
            count: 4,
            current,
            windows: vec![DeskWindow {
                desktop: 0,
                rect: [x, 0, 500, 500],
                focused: true,
                id: 1,
                app: "foot".into(),
            }],
        };
        let mut told = Told::default();
        assert_eq!(told.offer(view(0, 0), false), Some(view(0, 0)));
        // The same again (a frame of something else): nothing.
        assert_eq!(told.offer(view(0, 0), false), None);
        // Dragged: nothing on the way, where it lands at the end.
        assert_eq!(told.offer(view(0, 100), true), None);
        assert_eq!(told.offer(view(0, 200), true), None);
        assert_eq!(told.offer(view(0, 300), false), Some(view(0, 300)));
        // Back where it was told while held: nothing to say.
        assert_eq!(told.offer(view(0, 300), true), None);
        assert_eq!(told.offer(view(0, 300), false), None);
        // A desktop switch.
        assert_eq!(told.offer(view(2, 300), false), Some(view(2, 300)));
        // A shell connecting again is told everything.
        told.clear();
        assert_eq!(told.offer(view(2, 300), false), Some(view(2, 300)));
        told.forget("eDP-1");
        assert!(told.offer(view(2, 300), false).is_some());
        // Titles apart: a retitle is told alone, the desktops not again.
        let titles = |title: &str| Titles {
            output: "eDP-1".into(),
            titles: vec![(1, title.into())],
        };
        assert_eq!(told.offer_titles(titles("~")), Some(titles("~")));
        assert_eq!(told.offer_titles(titles("~")), None);
        assert_eq!(told.offer_titles(titles("~/src")), Some(titles("~/src")));
        assert_eq!(told.offer(view(2, 300), false), None);
    }

    #[test]
    fn with_reduced_motion_a_swipe_decides_on_release_and_fades() {
        let mut strip = Strip::new(4);
        strip.swipe_begin(ms(0), true);
        assert!(!strip.swipe_update(-SWIPE_DISTANCE / 2.0, 0.0, 10, ms(0)));
        assert_eq!(strip.position(ms(0)), 0.0);
        assert_eq!(strip.swipe_end(false, ms(0)), Some(0));
        assert_eq!(strip.alpha(1, ms(60)), 0.5);
    }
}
