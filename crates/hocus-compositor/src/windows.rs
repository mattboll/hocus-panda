//! The windows on screen (docs/design/windows.md §4–§10): where a new one
//! goes and whether it takes the focus, how each is snapped, maximised,
//! made fullscreen, minimised and brought back, and the motions that go
//! with each, driven by the frame clock.

use std::time::Duration;

use smithay::{
    desktop::Window,
    output::Output,
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Rectangle, SERIAL_COUNTER, Size},
    wayland::{
        compositor::with_states,
        seat::WaylandFocus,
        shell::xdg::{SurfaceCachedState, ToplevelSurface, XdgToplevelSurfaceData},
    },
};

use crate::{
    State,
    decoration::FrameIds,
    focus::KeyboardFocus,
    model::{self, Edges, Mode, Rect, Snap, SnapResult, WindowId, Zone},
    motion::{self, Bezier, Tween},
    snapshot::Snapshot,
};

/// How long a spawn shortcut lets the next new window take the focus.
const SPAWN_FOCUS: Duration = Duration::from_secs(10);

/// A window the desktop shows, and its motions under way.
#[derive(Debug)]
pub struct Managed {
    pub window: Window,
    pub ids: FrameIds,
    /// Opening: from 94 % and transparent.
    pub opening: Option<Tween>,
    /// Whether it shows as focused, and since when it changes to that.
    pub focused: bool,
    pub focus_change: Option<Tween>,
    /// Opened without the focus: its rim pulses once.
    pub pulse: Option<Tween>,
    /// Going to a new place: drawn from this rectangle (global) to where
    /// it is now.
    pub settle: Option<(Rectangle<f64, Logical>, Tween)>,
    /// The size last asked of its client, before it answers.
    pub requested: Option<Size<i32, Logical>>,
    /// Resized from these edges: the other ones stay where they are while
    /// its client takes the new size.
    pub resizing: Option<Resizing>,
    pub grip: Grip,
    /// Where it was before it went fullscreen.
    pub before_fullscreen: Mode,
    /// Where it is while it does not show (minimised, or on a desktop out
    /// of sight), global: where it comes back.
    pub away: Option<Rect>,
    /// Going to another desktop: drawn as if on this place of the strip
    /// (in desktops), moving to its desktop's (§12).
    pub travel: Option<(f64, Tween)>,
}

/// A resize under way, or ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resizing {
    pub edges: Edges,
    /// The far edges of this rectangle stay.
    pub anchor: Rect,
    /// The pointer let go: once the client has the last size, it is over.
    pub ended: bool,
}

/// The grip of a window with no header (§7).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Grip {
    #[default]
    Hidden,
    /// Appearing since then.
    Showing(Duration),
    /// The pointer left at that time: it stays a while, then fades.
    Leaving(Duration),
}

impl Grip {
    pub fn alpha(self, now: Duration) -> f32 {
        match self {
            Self::Hidden => 0.0,
            Self::Showing(since) => {
                Tween::new(since, motion::GRIP_IN, Bezier::EASE_OUT).at(now) as f32
            }
            Self::Leaving(since) => {
                let fade = since + motion::GRIP_STAYS;
                1.0 - Tween::new(fade, motion::GRIP_IN, Bezier::EASE_IN).at(now) as f32
            }
        }
    }

    /// What it becomes when the pointer comes (`wanted`) or goes, if it
    /// changes.
    pub fn towards(self, wanted: bool, now: Duration) -> Option<Self> {
        match (self, wanted) {
            (Self::Hidden | Self::Leaving(_), true) => {
                // Back before it faded: shown at once.
                let alpha = self.alpha(now);
                Some(Self::Showing(
                    now.saturating_sub(motion::GRIP_IN.mul_f64(f64::from(alpha))),
                ))
            }
            (Self::Showing(_), false) => Some(Self::Leaving(now)),
            _ => None,
        }
    }

    /// Faded out entirely.
    fn gone(self, now: Duration) -> bool {
        matches!(self, Self::Leaving(since) if now >= since + motion::GRIP_STAYS + motion::GRIP_IN)
    }

    /// Changing now, and if not, when it next will.
    fn moving(self, now: Duration) -> (bool, Option<Duration>) {
        match self {
            Self::Hidden => (false, None),
            Self::Showing(since) => (now < since + motion::GRIP_IN, None),
            Self::Leaving(since) => {
                let fade = since + motion::GRIP_STAYS;
                if now < fade {
                    (false, Some(fade))
                } else {
                    (now < fade + motion::GRIP_IN, None)
                }
            }
        }
    }
}

impl Managed {
    pub fn new(window: Window) -> Self {
        Self {
            window,
            ids: FrameIds::default(),
            opening: None,
            focused: false,
            focus_change: None,
            pulse: None,
            settle: None,
            requested: None,
            resizing: None,
            grip: Grip::Hidden,
            before_fullscreen: Mode::Floating,
            away: None,
            travel: None,
        }
    }

    /// How focused it looks at `now`, from 0 to 1.
    pub fn focus_look(&self, now: Duration) -> f32 {
        let t = self.focus_change.map_or(1.0, |t| t.at(now)) as f32;
        if self.focused { t } else { 1.0 - t }
    }

    /// The pulse's strength at `now`: up and back down, once.
    pub fn pulse_look(&self, now: Duration) -> f32 {
        self.pulse.map_or(0.0, |t| {
            let p = t.linear(now);
            (std::f64::consts::PI * p).sin().powi(2) as f32
        })
    }
}

/// A window gone or minimised: its last picture, moving and fading.
#[derive(Debug)]
pub struct Leaving {
    pub picture: Snapshot,
    /// Global.
    pub from: Rectangle<f64, Logical>,
    pub to: Rectangle<f64, Logical>,
    pub tween: Tween,
    pub fade: Tween,
}

/// Where a dragged window would snap, and since when the pointer is there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preview {
    pub zone: Zone,
    /// Global.
    pub rect: Rect,
    pub entered: Duration,
}

impl Preview {
    /// It shows once the pointer stayed in the zone a moment.
    pub fn shown(&self, now: Duration) -> bool {
        now >= self.entered + motion::PREVIEW_DELAY
    }

    pub fn alpha(&self, now: Duration) -> f32 {
        Tween::new(
            self.entered + motion::PREVIEW_DELAY,
            motion::PREVIEW,
            Bezier::EASE_OUT,
        )
        .at(now) as f32
    }
}

pub fn rect_of(r: Rectangle<i32, Logical>) -> Rect {
    Rect::new(r.loc.x, r.loc.y, r.size.w, r.size.h)
}

pub fn rectangle(r: Rect) -> Rectangle<i32, Logical> {
    Rectangle::new((r.x, r.y).into(), (r.width, r.height).into())
}

fn lerp_rect(
    a: Rectangle<f64, Logical>,
    b: Rectangle<f64, Logical>,
    t: f64,
) -> Rectangle<f64, Logical> {
    Rectangle::new(
        (
            motion::lerp(a.loc.x, b.loc.x, t),
            motion::lerp(a.loc.y, b.loc.y, t),
        )
            .into(),
        (
            motion::lerp(a.size.w, b.size.w, t),
            motion::lerp(a.size.h, b.size.h, t),
        )
            .into(),
    )
}

/// A window's application id: xdg-shell's, or X11's class.
pub fn app_id(window: &Window) -> Option<String> {
    if let Some(x11) = window.x11_surface() {
        return Some(x11.class());
    }
    let toplevel = window.toplevel()?;
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|d| d.lock().ok().and_then(|d| d.app_id.clone()))
    })
}

/// The smallest and largest sizes a window's client takes (0: none).
pub fn size_limits(window: &Window) -> ((i32, i32), (i32, i32)) {
    if let Some(x11) = window.x11_surface() {
        let min = x11.min_size().map_or((0, 0), |s| (s.w, s.h));
        let max = x11.max_size().map_or((0, 0), |s| (s.w, s.h));
        return (min, max);
    }
    let Some(toplevel) = window.toplevel() else {
        return ((0, 0), (0, 0));
    };
    with_states(toplevel.wl_surface(), |states| {
        let mut cached = states.cached_state.get::<SurfaceCachedState>();
        let current = cached.current();
        (
            (current.min_size.w, current.min_size.h),
            (current.max_size.w, current.max_size.h),
        )
    })
}

/// The parent process of `pid`, from `/proc`. Read on the loop: procfs
/// answers from memory, without waiting on a device.
fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command is in brackets and may hold spaces: the fields after it.
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(1)?.parse().ok()
}

/// Whether `pid` is `ancestor` or was started from it.
fn descends_from(pid: u32, ancestor: u32) -> bool {
    let mut at = pid;
    for _ in 0..32 {
        if at == ancestor {
            return true;
        }
        match parent_of(at) {
            Some(parent) if parent > 1 && parent != at => at = parent,
            _ => return false,
        }
    }
    false
}

impl State {
    pub fn now(&self) -> Duration {
        self.clock.now().into()
    }

    /// Whether motion is reduced (§16): the configuration's.
    pub fn reduced_motion(&self) -> bool {
        self.config.appearance.reduced_motion
    }

    /// A mapped window's geometry, global.
    pub fn rect_of(&self, id: WindowId) -> Option<Rect> {
        let managed = self.windows.get(&id)?;
        self.space.element_geometry(&managed.window).map(rect_of)
    }

    /// Where the window is, shown or not: where it comes back to when it
    /// does not show.
    pub fn home_rect(&self, id: WindowId) -> Option<Rect> {
        self.rect_of(id).or_else(|| self.windows.get(&id)?.away)
    }

    /// Where the window is going: its place, and the size asked of it.
    pub fn target_rect(&self, id: WindowId) -> Option<Rect> {
        let managed = self.windows.get(&id)?;
        let mut rect = self.home_rect(id)?;
        if let Some(size) = managed.requested {
            rect.width = size.w;
            rect.height = size.h;
        }
        Some(rect)
    }

    /// The output a window is on: the one under its centre, or the first.
    pub fn output_of(&self, id: WindowId) -> Option<Output> {
        let rect = self.home_rect(id)?;
        let (cx, cy) = rect.centre();
        self.space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| rect_of(g).contains(cx, cy))
            })
            .or_else(|| self.space.outputs().next())
            .cloned()
    }

    /// The work area of the window's output.
    pub fn area_of(&self, id: WindowId) -> Option<Rect> {
        self.work_area(&self.output_of(id)?)
    }

    /// Where it is drawn at `now`, global: where it goes, or on its way.
    pub fn visual_rect(&self, id: WindowId, now: Duration) -> Option<Rectangle<f64, Logical>> {
        let managed = self.windows.get(&id)?;
        let target = rectangle(self.target_rect(id)?).to_f64();
        Some(match managed.settle {
            Some((from, tween)) => lerp_rect(from, target, tween.at(now)),
            None => target,
        })
    }

    /// Whether the client draws its own header: no grip then.
    pub fn has_own_header(&self, id: WindowId) -> bool {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
        let Some(managed) = self.windows.get(&id) else {
            return true;
        };
        if let Some(x11) = managed.window.x11_surface() {
            return !x11.is_decorated();
        }
        managed
            .window
            .toplevel()
            .is_none_or(|t| t.current_state().decoration_mode != Some(DecorationMode::ServerSide))
    }

    /// Tells a window its size and states, and moves it to `rect`'s place
    /// (global); the size follows when the client takes it.
    pub fn configure(&mut self, id: WindowId, rect: Rect) {
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        let window = managed.window.clone();
        let size: Size<i32, Logical> = (rect.width.max(1), rect.height.max(1)).into();
        managed.requested = Some(size);
        let mode = self.model.mode(id);
        let resizing = managed.resizing.is_some_and(|r| !r.ended);
        if let Some(toplevel) = window.toplevel() {
            set_states(toplevel, Some(size), mode, resizing);
        }
        if let Some(x11) = window.x11_surface() {
            if let Err(err) = x11.configure(rectangle(rect)) {
                tracing::debug!(%err, "could not configure an X11 window");
            }
            let _ = x11.set_maximized(mode == Mode::Snapped(Zone::Maximized));
            let _ = x11.set_fullscreen(mode == Mode::Fullscreen);
        }
        if !self.model.is_shown(id) {
            // Out of sight: it comes back there.
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.away = Some(rect);
            }
        } else if !resizing {
            self.space.map_element(window, (rect.x, rect.y), false);
        }
        self.redraw = true;
    }

    /// Where a new window goes and whether it takes the focus; it opens
    /// animated.
    pub fn map_window(&mut self, window: Window) {
        let Some(output) = self.output_under_pointer() else {
            tracing::warn!("a window with no output to show on");
            self.unmapped.push(window);
            return;
        };
        let Some(area) = self.work_area(&output) else {
            self.unmapped.push(window);
            return;
        };
        let app = app_id(&window).unwrap_or_default();
        let mut size = window.geometry().size;
        // An application's last size, when it did not choose one larger.
        if let Some((w, h)) = self.model.remembered_size(&app)
            && window.toplevel().is_some()
            && (w, h) != (size.w, size.h)
        {
            size = (w.min(area.width), h.min(area.height)).into();
        }
        let same_app: Vec<(i32, i32)> = self
            .windows
            .iter()
            .filter(|(_, m)| app_id(&m.window).as_deref() == Some(app.as_str()))
            .filter_map(|(id, _)| self.rect_of(*id))
            .map(|r| (r.x, r.y))
            .collect();
        let (x, y) = model::place(area, (size.w, size.h), &same_app);
        let takes_focus = self.takes_focus(&window);
        let id = if takes_focus {
            self.model.add()
        } else {
            self.model.add_below_focus()
        };
        // On the desktop its screen shows.
        let desktop = self.strip(&output).current();
        self.model.set_desktop(id, desktop);
        let now = self.now();
        let mut managed = Managed::new(window.clone());
        let duration = if self.reduced_motion() {
            motion::FADE_REDUCED
        } else {
            motion::OPEN
        };
        managed.opening = Some(Tween::new(now, duration, Bezier::SPRING));
        if !takes_focus {
            managed.pulse = Some(Tween::new(now, motion::PULSE, Bezier::LINEAR));
        }
        self.windows.insert(id, managed);
        self.space.map_element(window.clone(), (x, y), false);
        let geometry = window.geometry().size;
        let rect = Rect::new(x, y, size.w, size.h);
        if window.x11_surface().is_some() || (size.w, size.h) != (geometry.w, geometry.h) {
            self.configure(id, rect);
        } else if let Some(toplevel) = window.toplevel() {
            set_states(toplevel, None, Mode::Floating, false);
        }
        tracing::debug!(
            window = id.0,
            app,
            x,
            y,
            w = size.w,
            h = size.h,
            takes_focus,
            "window mapped"
        );
        if takes_focus {
            self.focus(Some(id));
        } else {
            self.restack();
            self.sync_activated();
        }
        self.redraw = true;
    }

    /// Whether a new window takes the focus (§4): it does when the user
    /// started it, which shows as its application having the focus (a
    /// dialog, a new window of it), being started from the application
    /// with the focus (a terminal's command), or by the desktop's own
    /// shortcut a moment ago; and when nothing has the focus. Otherwise it
    /// opens behind, and its activation token, if it shows one, brings it
    /// forward.
    fn takes_focus(&mut self, window: &Window) -> bool {
        self.activate_on_map.retain(|s| s.is_alive());
        if let Some(surface) = window.wl_surface()
            && let Some(at) = self.activate_on_map.iter().position(|s| *s == *surface)
        {
            self.activate_on_map.remove(at);
            return true;
        }
        let now = self.now();
        // An X11 program the desktop started names its token.
        if let Some(token) = window.x11_surface().and_then(|x| x.startup_id())
            && let Some((made, valid)) = self.activation_tokens.remove(&token)
            && valid
            && now.saturating_sub(made) < SPAWN_FOCUS
        {
            return true;
        }
        let Some(focused) = self.model.focused().and_then(|id| self.windows.get(&id)) else {
            return true;
        };
        if self
            .spawned_at
            .take()
            .is_some_and(|at| now.saturating_sub(at) < SPAWN_FOCUS)
        {
            return true;
        }
        let pid_of = |w: &Window| -> Option<u32> {
            if let Some(x11) = w.x11_surface() {
                return x11.pid();
            }
            let surface = w.wl_surface()?;
            let client = self.display_handle.get_client(surface.id()).ok()?;
            let credentials = client.get_credentials(&self.display_handle).ok()?;
            u32::try_from(credentials.pid).ok()
        };
        let same_client = match (window.wl_surface(), focused.window.wl_surface()) {
            (Some(a), Some(b)) if window.x11_surface().is_none() => {
                a.id().same_client_as(&b.id()) && focused.window.x11_surface().is_none()
            }
            _ => false,
        };
        if same_client {
            return true;
        }
        match (pid_of(window), pid_of(&focused.window)) {
            (Some(new), Some(focused)) => new == focused || descends_from(new, focused),
            _ => false,
        }
    }

    /// A window went, or its client hid it: it fades from its last frame.
    pub fn unmap_window(&mut self, window: &Window) {
        self.unmapped.retain(|w| w != window);
        let Some(id) = self.id_of(window) else {
            return;
        };
        self.leave(id, false);
        if let Some(app) = app_id(window)
            && let Some(rect) = self.rect_of(id)
            && self.model.mode(id) == Mode::Floating
        {
            self.model.remember_size(&app, (rect.width, rect.height));
        }
        self.end_popup_grab_of(id);
        self.switcher_forget(id);
        self.overlay.forget(id);
        self.windows.remove(&id);
        self.pictures.remove(&id);
        self.space.unmap_elem(window);
        if self.wobble.as_ref().is_some_and(|(w, _)| *w == id) {
            self.wobble = None;
        }
        if self.moving == Some(id) {
            self.moving = None;
        }
        let had_focus = self.model.focused() == Some(id);
        self.model.remove(id);
        if had_focus {
            let next = self.model.focused();
            self.focus(next);
        }
        self.redraw = true;
    }

    /// The window's last picture, as it is drawn or as its surface went.
    fn picture_of(&self, id: WindowId) -> Option<Snapshot> {
        let managed = self.windows.get(&id)?;
        let window = &managed.window;
        let geometry = window.geometry();
        window
            .wl_surface()
            .and_then(|s| self.grabber.take(&s, geometry.loc, geometry.size))
            .or_else(|| self.pictures.get(&id).cloned())
    }

    /// A window's surface is going: its picture, while its textures are
    /// still there, in case its toplevel goes after.
    pub fn surface_going(&mut self, surface: &WlSurface) {
        let Some(id) = self
            .windows
            .iter()
            .find(|(_, m)| m.window.wl_surface().is_some_and(|s| *s == *surface))
            .map(|(id, _)| *id)
        else {
            return;
        };
        if let Some(picture) = self.picture_of(id) {
            self.pictures.insert(id, picture);
        }
    }

    /// Its picture leaves: closing (shrinking a little and fading), or
    /// minimised (toward the bar's place).
    fn leave(&mut self, id: WindowId, minimizing: bool) {
        if !self.model.is_shown(id) {
            return;
        }
        let now = self.now();
        let Some(picture) = self.picture_of(id) else {
            return;
        };
        let Some(from) = self.visual_rect(id, now) else {
            return;
        };
        let reduced = self.reduced_motion();
        let to = if reduced {
            from
        } else if minimizing {
            self.bar_place(id).unwrap_or(from)
        } else {
            let k = motion::CLOSE_SCALE;
            let size = from.size.upscale(k);
            let loc = from.loc + (from.size.to_point() - size.to_point()).downscale(2.0);
            Rectangle::new(loc, size)
        };
        let (duration, curve) = match (reduced, minimizing) {
            (true, _) => (motion::FADE_REDUCED, Bezier::LINEAR),
            (false, true) => (motion::MINIMIZE, Bezier::EASE_IN),
            (false, false) => (motion::CLOSE, Bezier::EASE_IN),
        };
        let tween = Tween::new(now, duration, curve);
        self.leaving.push(Leaving {
            picture,
            from,
            to,
            tween,
            fade: tween,
        });
        self.redraw = true;
    }

    /// Where the bar is on the window's output, as a small box: where a
    /// minimised window goes.
    fn bar_place(&self, id: WindowId) -> Option<Rectangle<f64, Logical>> {
        let output = self.output_of(id)?;
        let geometry = self.space.output_geometry(&output)?;
        let bar = f64::from(model::bar_width(geometry.size.w));
        let x = f64::from(geometry.loc.x + geometry.size.w) - f64::from(model::GAP) - bar / 2.0;
        let y = f64::from(geometry.loc.y) + f64::from(geometry.size.h) / 2.0;
        Some(Rectangle::new(
            (x - 24.0, y - 18.0).into(),
            (48.0, 36.0).into(),
        ))
    }

    /// Puts smithay's stacking in the model's order.
    pub fn restack(&mut self) {
        for id in self.model.stack().to_vec() {
            if let Some(managed) = self.windows.get(&id) {
                let window = managed.window.clone();
                self.space.raise_element(&window, false);
                if let (Some(x11), Some(xwm)) = (window.x11_surface(), self.xwm.as_mut())
                    && let Err(err) = xwm.raise_window(x11)
                {
                    tracing::debug!(%err, "could not raise an X11 window");
                }
            }
        }
    }

    /// Tells every window whether it is the active one, and starts their
    /// rims' change.
    fn sync_activated(&mut self) {
        let focused = self.model.focused();
        let now = self.now();
        for (wid, managed) in &mut self.windows {
            let active = Some(*wid) == focused;
            if managed.focused != active {
                managed.focused = active;
                managed.focus_change = Some(Tween::new(now, motion::FOCUS_RIM, Bezier::EASE_OUT));
            }
            let window = &managed.window;
            if window.set_activated(active)
                && let Some(toplevel) = window.toplevel()
                && toplevel.is_initial_configure_sent()
            {
                toplevel.send_pending_configure();
            }
            if let Some(x11) = window.x11_surface()
                && let Err(err) = x11.set_activated(active)
            {
                tracing::debug!(%err, "could not tell an X11 window it is active");
            }
        }
    }

    /// Focuses and raises a window (none: no window has the keyboard), and
    /// tells every window whether it is the active one.
    pub fn focus(&mut self, id: Option<WindowId>) {
        if id != self.model.focused() {
            self.end_popup_grab();
        }
        self.model.focus(id);
        let focused = self.model.focused();
        self.sync_activated();
        if let Some(id) = focused
            && let Some(managed) = self.windows.get_mut(&id)
        {
            managed.pulse = None;
        }
        self.restack();
        let target = focused
            .and_then(|id| self.windows.get(&id))
            .map(|m| m.window.clone());
        let focus = self.popup_focus().or_else(|| {
            target.and_then(|w| match w.x11_surface() {
                Some(x11) => Some(KeyboardFocus::X11(x11.clone())),
                None => w
                    .toplevel()
                    .map(|t| KeyboardFocus::Wayland(t.wl_surface().clone())),
            })
        });
        // A lock screen or a launcher holding the keyboard keeps it.
        if !self.keyboard_held_by_layer()
            && let Some(keyboard) = self.seat.get_keyboard()
        {
            keyboard.set_focus(self, focus, SERIAL_COUNTER.next_serial());
        }
        self.redraw = true;
    }

    /// Snaps `id` to `zone` of its output's work area, from where it is
    /// drawn now.
    pub fn snap(&mut self, id: WindowId, zone: Zone) {
        self.snap_from(id, zone, None);
    }

    /// Snaps `id`; `floating`: where it goes back to when restored, if not
    /// where it is (a drag: where the drag began).
    pub fn snap_from(&mut self, id: WindowId, zone: Zone, floating: Option<Rect>) {
        let (Some(area), Some(current)) = (self.area_of(id), self.rect_of(id)) else {
            return;
        };
        let current = floating.unwrap_or(current);
        self.start_settle(id);
        let mode = self.model.mode(id);
        let floating = match mode {
            Mode::Floating => current,
            Mode::Snapped(_) | Mode::Fullscreen => {
                self.model.placement(id).restore.unwrap_or(current)
            }
        };
        if mode == Mode::Fullscreen {
            self.model.unsnap(id);
        }
        self.model.set_mode(id, Mode::Snapped(zone), floating);
        self.configure(id, zone.rect(area));
    }

    /// Back where it floated before it snapped, was maximised or went
    /// fullscreen.
    pub fn restore(&mut self, id: WindowId) {
        let Some(current) = self.rect_of(id) else {
            return;
        };
        self.start_settle(id);
        let rect = self.model.unsnap(id).unwrap_or(current);
        self.configure(id, rect);
    }

    /// Fullscreen over its whole output, or back.
    pub fn set_fullscreen(&mut self, id: WindowId, fullscreen: bool) {
        let mode = self.model.mode(id);
        tracing::debug!(window = id.0, fullscreen, ?mode, "fullscreen");
        if fullscreen == (mode == Mode::Fullscreen) {
            // Still tell it: a client waits for a configure.
            if let Some(rect) = self.target_rect(id) {
                self.configure(id, rect);
            }
            return;
        }
        let (Some(output), Some(current)) = (self.output_of(id), self.rect_of(id)) else {
            return;
        };
        self.start_settle(id);
        if fullscreen {
            let Some(geometry) = self.space.output_geometry(&output) else {
                return;
            };
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.before_fullscreen = mode;
            }
            self.model.set_mode(id, Mode::Fullscreen, current);
            self.configure(id, rect_of(geometry));
            self.focus(Some(id));
        } else {
            let before = self
                .windows
                .get(&id)
                .map_or(Mode::Floating, |m| m.before_fullscreen);
            match before {
                Mode::Snapped(zone) => {
                    // Back in its zone; where it floated before is kept.
                    self.model.set_mode(id, Mode::Snapped(zone), current);
                    if let Some(area) = self.area_of(id) {
                        self.configure(id, zone.rect(area));
                    }
                }
                Mode::Floating | Mode::Fullscreen => {
                    let rect = self.model.unsnap(id).unwrap_or(current);
                    self.configure(id, rect);
                }
            }
        }
    }

    /// Maximised, or back.
    pub fn toggle_maximized(&mut self, id: WindowId) {
        if self.model.mode(id) == Mode::Snapped(Zone::Maximized) {
            self.restore(id);
        } else {
            self.snap(id, Zone::Maximized);
        }
    }

    /// The window's picture shrinks away; the window is kept, unmapped,
    /// until it comes back (§10).
    pub fn minimize(&mut self, id: WindowId) {
        if self.model.is_minimized(id) {
            return;
        }
        let Some(rect) = self.rect_of(id) else {
            return;
        };
        self.leave(id, true);
        self.end_popup_grab_of(id);
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        managed.away = Some(rect);
        managed.settle = None;
        let window = managed.window.clone();
        if let Some(toplevel) = window.toplevel() {
            // Nothing of it shows: it may stop drawing.
            toplevel.with_pending_state(|s| {
                s.states.set(xdg_toplevel::State::Suspended);
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.space.unmap_elem(&window);
        if self.wobble.as_ref().is_some_and(|(w, _)| *w == id) {
            self.wobble = None;
        }
        let had_focus = self.model.focused() == Some(id);
        self.model.minimize(id);
        if had_focus {
            let next = self.model.focused();
            self.focus(next);
        }
        self.redraw = true;
    }

    /// A minimised window comes back where it was, focused; on a desktop
    /// out of sight, it waits there.
    pub fn unminimize(&mut self, id: WindowId) {
        if !self.model.is_minimized(id) {
            return;
        }
        self.model.unminimize(id);
        if !self.model.is_shown(id) {
            return;
        }
        let now = self.now();
        let reduced = self.reduced_motion();
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        let rect = managed.away.take().unwrap_or_default();
        let window = managed.window.clone();
        managed.opening = Some(Tween::new(
            now,
            if reduced {
                motion::FADE_REDUCED
            } else {
                motion::OPEN
            },
            Bezier::SPRING,
        ));
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Suspended);
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.space.map_element(window, (rect.x, rect.y), false);
        self.focus(Some(id));
    }

    /// The window minimised last comes back.
    pub fn unminimize_last(&mut self) {
        if let Some(id) = self.model.last_minimized() {
            self.unminimize(id);
        }
    }

    /// Asks the focused window to close: the application decides.
    pub fn close_focused(&mut self) {
        let Some(managed) = self.model.focused().and_then(|id| self.windows.get(&id)) else {
            return;
        };
        if let Some(toplevel) = managed.window.toplevel() {
            toplevel.send_close();
        } else if let Some(x11) = managed.window.x11_surface()
            && let Err(err) = x11.close()
        {
            tracing::debug!(%err, "could not ask an X11 window to close");
        }
    }

    /// The keys of §9 on the focused window.
    pub fn keyboard_snap(&mut self, snap: Snap) {
        let Some(id) = self.model.focused() else {
            return;
        };
        let (Some(rect), Some(area)) = (self.rect_of(id), self.area_of(id)) else {
            return;
        };
        match model::keyboard_snap(snap, self.model.mode(id), rect, area) {
            SnapResult::To(zone) => self.snap(id, zone),
            SnapResult::Restore => {
                if self.model.mode(id) == Mode::Fullscreen {
                    self.set_fullscreen(id, false);
                } else {
                    self.restore(id);
                }
            }
            SnapResult::Minimize => self.minimize(id),
        }
    }

    /// Focuses the nearest window on that side of the focused one (§4).
    pub fn focus_toward(&mut self, dx: i32, dy: i32) {
        let Some(from) = self.model.focused().and_then(|id| self.rect_of(id)) else {
            return;
        };
        let (fx, fy) = from.centre();
        let best = self
            .windows
            .keys()
            .copied()
            .filter(|id| Some(*id) != self.model.focused() && !self.model.is_minimized(*id))
            .filter_map(|id| {
                let (x, y) = self.rect_of(id)?.centre();
                let (ddx, ddy) = (x - fx, y - fy);
                let along = ddx * f64::from(dx) + ddy * f64::from(dy);
                let across = (ddx * f64::from(dy)).abs() + (ddy * f64::from(dx)).abs();
                (along > 0.0).then_some((id, along + 2.0 * across))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        if best.is_some() {
            self.focus(best);
        }
    }

    /// Starts drawing the window from where it shows now toward wherever
    /// it goes next.
    pub fn start_settle(&mut self, id: WindowId) {
        let now = self.now();
        let Some(from) = self.visual_rect(id, now) else {
            return;
        };
        let reduced = self.reduced_motion();
        if self.wobble.as_ref().is_some_and(|(w, _)| *w == id) {
            self.wobble = None;
        }
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.settle =
                (!reduced).then(|| (from, Tween::new(now, motion::SETTLE, Bezier::SPRING)));
        }
        self.redraw = true;
    }

    /// The window's client committed: a window resized from its top or
    /// left keeps its far edges where they were.
    pub fn window_committed(&mut self, id: WindowId) {
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        let size = managed.window.geometry().size;
        if let Some(resizing) = managed.resizing {
            let anchor = resizing.anchor;
            let x = if resizing.edges.left {
                anchor.right() - size.w
            } else {
                anchor.x
            };
            let y = if resizing.edges.top {
                anchor.bottom() - size.h
            } else {
                anchor.y
            };
            let done = resizing.ended && managed.requested == Some(size);
            if done {
                managed.resizing = None;
            }
            let window = managed.window.clone();
            if self.space.element_location(&window) != Some((x, y).into()) {
                self.space.map_element(window, (x, y), false);
            }
        }
    }

    /// The motions under way move on; finished ones go. Asks for the next
    /// frame while something moves, and for a wake-up when something will
    /// start moving after a pause.
    pub fn tick(&mut self) {
        let now = self.now();
        let mut moving = false;
        let mut next: Option<Duration> = None;
        let mut wake = |at: Duration| next = Some(next.map_or(at, |n: Duration| n.min(at)));
        for managed in self.windows.values_mut() {
            for tween in [
                &mut managed.opening,
                &mut managed.focus_change,
                &mut managed.pulse,
            ] {
                if let Some(t) = tween {
                    if t.done(now) {
                        *tween = None;
                    } else {
                        moving = true;
                    }
                }
            }
            if let Some((_, t)) = managed.settle {
                if t.done(now) {
                    managed.settle = None;
                } else {
                    moving = true;
                }
            }
            if let Some((_, t)) = managed.travel {
                if t.done(now) {
                    managed.travel = None;
                } else {
                    moving = true;
                }
            }
            let (grip_moving, grip_next) = managed.grip.moving(now);
            moving |= grip_moving;
            if let Some(at) = grip_next {
                wake(at);
            }
            if managed.grip.gone(now) {
                managed.grip = Grip::Hidden;
            }
        }
        if let Some((_, handle)) = self.seam_handle {
            let (handle_moving, handle_next) = handle.moving(now);
            moving |= handle_moving;
            if let Some(at) = handle_next {
                wake(at);
            }
            if handle.gone(now) {
                self.seam_handle = None;
            }
        }
        for strip in self.strips.values_mut() {
            moving |= strip.tick(now);
        }
        moving |= self.tick_switcher(now, &mut wake);
        self.leaving.retain(|l| !l.tween.done(now));
        moving |= !self.leaving.is_empty();
        if let Some(preview) = &self.preview {
            if preview.shown(now) {
                moving |= preview.alpha(now) < 1.0;
            } else {
                wake(preview.entered + motion::PREVIEW_DELAY);
            }
        }
        if let Some((_, t)) = &self.preview_out {
            if t.done(now) {
                self.preview_out = None;
            } else {
                moving = true;
            }
        }
        if let Some((_, jelly)) = &mut self.wobble {
            jelly.advance(now);
            if jelly.settled() {
                self.wobble = None;
            } else {
                moving = true;
            }
        }
        if moving {
            self.redraw = true;
        } else if let Some(at) = next {
            self.wake_at(at);
        }
    }

    /// A frame at `at`, after a pause in which nothing moves.
    fn wake_at(&mut self, at: Duration) {
        if self.wake.is_some_and(|w| w <= at) {
            return;
        }
        let delay = at.saturating_sub(self.now());
        let timer = Timer::from_duration(delay);
        let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
            app.state.wake = None;
            app.state.redraw = true;
            TimeoutAction::Drop
        });
        match inserted {
            Ok(_) => self.wake = Some(at),
            Err(err) => tracing::warn!(%err.error, "could not schedule a frame"),
        }
    }

    /// Shows where a dragged window would snap, or nothing.
    pub fn set_preview(&mut self, zone: Option<(Zone, Rect)>) {
        let now = self.now();
        let same = match (&self.preview, zone) {
            (Some(p), Some((z, _))) => p.zone == z,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(old) = self.preview.take()
            && old.shown(now)
        {
            let alpha = f64::from(old.alpha(now));
            // Fades from where it got to.
            let start = now.saturating_sub(motion::PREVIEW.mul_f64(1.0 - alpha));
            self.preview_out = Some((
                old.rect,
                Tween::new(start, motion::PREVIEW, Bezier::EASE_IN),
            ));
        }
        self.preview = zone.map(|(zone, rect)| Preview {
            zone,
            rect,
            entered: now,
        });
        self.redraw = true;
    }

    /// The window under the pointer shows its grip when the pointer is in
    /// its top band; the others hide theirs.
    pub fn hover_grip(&mut self, over: Option<WindowId>) {
        let now = self.now();
        let mut changed = false;
        for (id, managed) in &mut self.windows {
            if let Some(grip) = managed.grip.towards(over == Some(*id), now) {
                managed.grip = grip;
                changed = true;
            }
        }
        if changed {
            self.redraw = true;
        }
    }

    /// The seams of snapped pairs (§8): snapped windows, not maximised,
    /// side by side or one above the other on the same output.
    pub fn seams(&self) -> Vec<model::Seam> {
        let snapped: Vec<(WindowId, Rect, Option<Output>)> = self
            .model
            .stack()
            .iter()
            .copied()
            .filter(|id| !self.model.is_minimized(*id))
            .filter(|id| matches!(self.model.mode(*id), Mode::Snapped(z) if z != Zone::Maximized))
            .filter_map(|id| Some((id, self.rect_of(id)?, self.output_of(id))))
            .collect();
        let mut seams = Vec::new();
        for (i, (a, ra, oa)) in snapped.iter().enumerate() {
            for (b, rb, ob) in &snapped[i + 1..] {
                if oa == ob
                    && let Some(seam) = model::seam((*a, *ra), (*b, *rb))
                {
                    seams.push(seam);
                }
            }
        }
        seams
    }

    /// The seam between `first` and `second` as they are now, if they
    /// still face each other.
    pub fn live_seam(&self, first: WindowId, second: WindowId) -> Option<model::Seam> {
        self.seams()
            .into_iter()
            .find(|s| s.first == first && s.second == second)
    }

    /// The handle of the seam the pointer is near shows; another one goes.
    pub fn hover_seam(&mut self, near: Option<model::Seam>) {
        let now = self.now();
        let current = self
            .seam_handle
            .filter(|(s, _)| self.live_seam(s.first, s.second).is_some());
        let next = match (current, near) {
            (Some((s, handle)), Some(n)) if (s.first, s.second) == (n.first, n.second) => {
                Some((n, handle.towards(true, now).unwrap_or(handle)))
            }
            (_, Some(n)) => Some((n, Grip::Showing(now))),
            (Some((s, handle)), None) => Some((s, handle.towards(false, now).unwrap_or(handle))),
            (None, None) => None,
        };
        if next != self.seam_handle {
            self.seam_handle = next;
            self.redraw = true;
        }
    }

    /// Activation asked with a token the desktop or the user's input
    /// gave: the window comes forward, back from minimised. Otherwise its
    /// rim pulses once.
    pub fn activate(&mut self, id: WindowId, allowed: bool) {
        if allowed {
            self.bring(id);
        } else if self.model.focused() != Some(id) {
            let now = self.now();
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.pulse = Some(Tween::new(now, motion::PULSE, Bezier::LINEAR));
            }
            self.redraw = true;
        }
    }

    /// The outputs' work areas changed (a layer's exclusive zone, a mode):
    /// snapped windows follow.
    pub fn rearrange(&mut self) {
        let ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in ids {
            if self.model.is_minimized(id) {
                continue;
            }
            match self.model.mode(id) {
                Mode::Snapped(zone) => {
                    if let Some(area) = self.area_of(id) {
                        let rect = zone.rect(area);
                        if self.target_rect(id) != Some(rect) {
                            self.configure(id, rect);
                        }
                    }
                }
                Mode::Fullscreen => {
                    if let Some(g) = self
                        .output_of(id)
                        .and_then(|o| self.space.output_geometry(&o))
                        && self.target_rect(id) != Some(rect_of(g))
                    {
                        self.configure(id, rect_of(g));
                    }
                }
                Mode::Floating => {}
            }
        }
    }
}

/// A toplevel's size and states: tiled when snapped, maximised,
/// fullscreen and resizing as it is.
pub fn set_states(
    toplevel: &ToplevelSurface,
    size: Option<Size<i32, Logical>>,
    mode: Mode,
    resizing: bool,
) {
    use xdg_toplevel::State as S;
    toplevel.with_pending_state(|s| {
        if let Some(size) = size {
            s.size = Some(size);
        }
        // Snapped: tiled, so that its client draws no shadow nor rounded
        // corners of its own. Floating, it may size itself (a client
        // takes no size of its own while tiled).
        let tiled = matches!(mode, Mode::Snapped(_));
        for side in [S::TiledLeft, S::TiledRight, S::TiledTop, S::TiledBottom] {
            if tiled {
                s.states.set(side);
            } else {
                s.states.unset(side);
            }
        }
        let toggle = |states: &mut smithay::wayland::shell::xdg::ToplevelStateSet, st, on: bool| {
            if on {
                states.set(st);
            } else {
                states.unset(st);
            }
        };
        toggle(
            &mut s.states,
            S::Maximized,
            mode == Mode::Snapped(Zone::Maximized),
        );
        toggle(&mut s.states, S::Fullscreen, mode == Mode::Fullscreen);
        toggle(&mut s.states, S::Resizing, resizing);
    });
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}
