//! Idle, the curtain and the lock (#29). Without input, the curtain dims
//! every screen with a countdown, then the session locks, and later the
//! machine sleeps if asked to; on battery, the delays of `[idle.battery]`
//! apply. Idle inhibitors and fullscreen windows (unless told otherwise)
//! hold it off, and the countdown starts again when they stop. Locked, the
//! screens show only the lock: a lock client's surfaces
//! (ext-session-lock), or our own lock screen, which checks the
//! password through PAM on a worker thread. A lock client that goes away
//! without unlocking leaves our lock screen: a crash never unlocks.
//!
//! Nor does the compositor's own: while locked, a file in the state
//! directory says so, and a session that starts and finds it (the last one
//! crashed or was killed while locked, and an autologin started this one)
//! starts locked.

use std::{collections::BTreeMap, time::Duration};

use hocus_api::UiNode;
use hocus_platform::auth::{Auth, Checker, Pam};
use hocus_shell::{
    lock::{Check, Curtain, LockScreen},
    matrix::Variant,
};
use hocus_ui::{
    Key, Size as UiSize,
    motion::{Animated, Curve, Easing, LARGE, STANDARD},
};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            Kind,
            memory::MemoryRenderBufferRenderElement,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
    },
    delegate_session_lock,
    input::keyboard::{Keysym, ModifiersState},
    output::Output,
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken, channel,
            timer::{TimeoutAction, Timer},
        },
        wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1,
        wayland_server::{Resource, protocol::wl_output::WlOutput},
    },
    utils::{Logical, Physical, Point, Rectangle, SERIAL_COUNTER, Scale, Size},
    wayland::session_lock::{
        LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
    },
};

use crate::{App, State, handlers::protocols::trusted, shell::Panel};

/// Where the session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Unlocked,
    /// The curtain dims the screens since then.
    Curtain {
        since: Duration,
    },
    Locked,
}

/// Who draws the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
enum By {
    Us,
    /// A lock client, while its lock object lives.
    Client(ExtSessionLockV1),
}

/// The password checked in tests (the virtual clock never runs a real
/// session).
struct TestChecker;

impl Checker for TestChecker {
    fn check(&mut self, _: &str, password: &str) -> bool {
        password == "panda"
    }
}

pub struct Locking {
    pub phase: Phase,
    by: By,
    /// The last input, or the last moment something held idle off.
    last_activity: Duration,
    /// The last input alone, for idle notifications that ignore holds.
    last_input: Duration,
    /// The machine was put to sleep for the idle that began then: once
    /// for each.
    slept_for: Option<Duration>,
    state: SessionLockManagerState,
    lock_surfaces: BTreeMap<String, LockSurface>,
    curtains: BTreeMap<String, Panel<Curtain>>,
    screens: BTreeMap<String, Panel<LockScreen>>,
    auth: Option<Auth>,
    logind: Option<hocus_platform::session::Logind>,
    screensaver: Option<hocus_platform::session::ScreenSaverService>,
    /// Applications keeping the screen awake through D-Bus.
    dbus_inhibitors: u32,
    /// logind's signals followed, for the test harness (#235).
    pub(crate) logind_following: usize,
    /// logind was told the lock shows.
    told_locked: bool,
    /// The timer that wakes the loop at the next deadline, and when.
    timer: Option<(RegistrationToken, Duration)>,
    /// Writes and removes the locked marker, in order, off the loop.
    marker: Option<std::sync::mpsc::Sender<bool>>,
    /// Our lock screen coming over the desktop it hides (#48).
    entering: Option<Moment>,
    /// Unlocked: our lock screens going from the desktop.
    parting: Option<(Moment, BTreeMap<String, Panel<LockScreen>>)>,
    /// The Matrix theme: when a password was last refused, for the lock to
    /// shake (#155).
    pub(crate) refused: Option<Duration>,
}

/// How our lock screen comes and goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Moment {
    /// Fading in; unlocked, its halves parting from the centre (0 to 1).
    Plain(Animated),
    /// In the Matrix theme, since then: rain cascades over the desktop;
    /// unlocked, it speeds up and flashes (#155).
    Matrix(Duration),
}

impl Moment {
    pub(crate) fn is_running(&self, now: Duration, total: Duration) -> bool {
        match self {
            Self::Plain(a) => a.is_running(now),
            Self::Matrix(since) => now.saturating_sub(*since) < total,
        }
    }
}

impl std::fmt::Debug for Locking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Locking")
            .field("phase", &self.phase)
            .field("by", &self.by)
            .finish_non_exhaustive()
    }
}

impl Locking {
    pub fn new(dh: &smithay::reexports::wayland_server::DisplayHandle) -> Self {
        Self {
            phase: Phase::Unlocked,
            by: By::Us,
            last_activity: Duration::ZERO,
            last_input: Duration::ZERO,
            slept_for: None,
            state: SessionLockManagerState::new::<State, _>(dh, trusted),
            lock_surfaces: BTreeMap::new(),
            curtains: BTreeMap::new(),
            screens: BTreeMap::new(),
            auth: None,
            logind: None,
            screensaver: None,
            dbus_inhibitors: 0,
            logind_following: 0,
            told_locked: false,
            timer: None,
            marker: None,
            entering: None,
            parting: None,
            refused: None,
        }
    }
}

/// Starts the password service; its answers come back to the loop.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (done, results) = channel::channel::<bool>();
    if let Err(err) = handle.insert_source(results, |event, _, app| {
        if let channel::Event::Msg(ok) = event {
            app.timed("password", |app| app.state.password_checked(ok));
        }
    }) {
        tracing::warn!(err = %err.error, "no password checks");
        return;
    }
    let checker: Box<dyn Checker> = match state.clock {
        crate::clock::Clock::Virtual { .. } => Box::new(TestChecker),
        crate::clock::Clock::Monotonic { .. } => Box::new(Pam),
    };
    match Auth::start(checker, move |ok| {
        let _ = done.send(ok);
    }) {
        Ok(auth) => state.locking.auth = Some(auth),
        Err(err) => tracing::warn!(%err, "no password checks"),
    }
}

/// Keeps the locked state in `dir`, and locks now if the last session
/// ended locked. Read once at start, before the first frame.
pub fn persist(state: &mut State, dir: Option<std::path::PathBuf>) {
    let Some(dir) = dir else {
        return;
    };
    let path = dir.join("locked");
    let was_locked = path.exists();
    let (marker, wanted) = std::sync::mpsc::channel::<bool>();
    let spawned = std::thread::Builder::new()
        .name("lock marker".into())
        .spawn(move || {
            while let Ok(locked) = wanted.recv() {
                let done = if locked {
                    std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, b""))
                } else {
                    match std::fs::remove_file(&path) {
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        other => other,
                    }
                };
                if let Err(err) = done {
                    tracing::warn!(%err, locked, "the locked state was not written");
                }
            }
        });
    match spawned {
        Ok(_) => state.locking.marker = Some(marker),
        Err(err) => tracing::warn!(%err, "the locked state will not survive a crash"),
    }
    if was_locked {
        tracing::warn!("the last session ended locked: locking");
        state.lock_now();
    }
}

impl State {
    pub fn locked(&self) -> bool {
        self.locking.phase == Phase::Locked
    }

    /// Input happened: the countdown starts again, the curtain lifts.
    pub fn activity(&mut self) {
        let now = self.clock.now();
        self.locking.last_activity = now;
        self.locking.last_input = now;
        self.idle_resumed();
        if matches!(self.locking.phase, Phase::Curtain { .. }) {
            self.locking.phase = Phase::Unlocked;
            self.needs_redraw = true;
        }
    }

    /// The charger was plugged or unplugged: someone is there, and the
    /// delays of the other source count from now.
    pub fn power_source_changed(&mut self, source: hocus_core::config::PowerSource) {
        tracing::info!(?source, "power source changed: its idle delays apply");
        self.locking.last_activity = self.clock.now();
        if matches!(self.locking.phase, Phase::Curtain { .. }) {
            self.locking.phase = Phase::Unlocked;
        }
        self.needs_redraw = true;
    }

    /// The last activity (holds included) and the last input.
    pub fn idle_since(&self) -> (Duration, Duration) {
        (self.locking.last_activity, self.locking.last_input)
    }

    /// Locks now, with our lock screen.
    pub fn lock_now(&mut self) {
        if self.locked() {
            return;
        }
        tracing::info!("locking");
        self.locking.phase = Phase::Locked;
        self.locking.by = By::Us;
        if self.matrix_moves() {
            self.locking.entering = Some(Moment::Matrix(self.clock.now()));
        } else if self.lock_moves() {
            let mut fade = Animated::new(0.0);
            fade.set(
                1.0,
                Curve::Ease(Easing::OutCubic, STANDARD),
                self.clock.now(),
            );
            self.locking.entering = Some(Moment::Plain(fade));
        }
        self.lock_began();
    }

    /// What every lock does: no window keeps the keyboard or the pointer,
    /// the shell's panels close, and the lock is written down.
    fn lock_began(&mut self) {
        if let Some(marker) = &self.locking.marker {
            let _ = marker.send(true);
        }
        self.close_launcher();
        self.close_quick();
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        for screen in self.locking.screens.values_mut() {
            screen.view.set_check(Check::Typing);
        }
        self.lock_keyboard();
        self.needs_redraw = true;
    }

    fn unlock(&mut self) {
        tracing::info!("unlocked");
        if let Some(marker) = &self.locking.marker {
            let _ = marker.send(false);
        }
        let ours = self.locking.by == By::Us;
        self.locking.phase = Phase::Unlocked;
        self.locking.by = By::Us;
        self.locking.lock_surfaces.clear();
        self.locking.last_activity = self.clock.now();
        self.locking.entering = None;
        self.locking.refused = None;
        let screens = std::mem::take(&mut self.locking.screens);
        // Ours parts from the centre on the desktop it hid; in the Matrix
        // theme, its rain speeds up and flashes.
        if ours && self.matrix_moves() && !screens.is_empty() {
            self.locking.parting = Some((Moment::Matrix(self.clock.now()), screens));
        } else if ours && self.lock_moves() && !screens.is_empty() {
            let mut open = Animated::new(0.0);
            open.set(
                1.0,
                Curve::Ease(Easing::InOutCubic, LARGE),
                self.clock.now(),
            );
            self.locking.parting = Some((Moment::Plain(open), screens));
        }
        self.apply_focus(SERIAL_COUNTER.next_serial());
        self.needs_redraw = true;
    }

    fn password_checked(&mut self, ok: bool) {
        if !self.locked() {
            return;
        }
        if ok {
            self.unlock();
        } else {
            for screen in self.locking.screens.values_mut() {
                screen.view.set_check(Check::Failed);
            }
            if self.matrix_moves() {
                self.locking.refused = Some(self.clock.now());
            }
            self.needs_redraw = true;
        }
    }

    /// Whether a lock client draws the lock now.
    fn client_lock(&self) -> bool {
        matches!(&self.locking.by, By::Client(lock) if lock.is_alive())
    }

    /// Locked, keys go to the lock: a lock client's surface, or our lock
    /// screen. Whether `sym` was ours.
    pub fn lock_key(&mut self, sym: Keysym, modifiers: &ModifiersState) -> bool {
        if !self.locked() || self.client_lock() {
            return false;
        }
        let user = self.user.clone();
        let mut password = None;
        for screen in self.locking.screens.values_mut() {
            match crate::launcher::key_of(sym) {
                Some(key) => {
                    if let Some(p) = screen.view.key(key)
                        && password.is_none()
                    {
                        password = Some(p);
                    }
                }
                None if !modifiers.ctrl && !modifiers.alt && !modifiers.logo => {
                    if let Some(c) = sym.key_char().filter(|c| !c.is_control()) {
                        screen.view.text(c.encode_utf8(&mut [0; 4]));
                    }
                }
                None => {}
            }
        }
        if let (Some(password), Some(auth)) = (password, &self.locking.auth) {
            // Every screen shows the same field: they are all checking.
            for screen in self.locking.screens.values_mut() {
                screen.view.set_check(Check::Checking);
                screen.view.key(Key::Escape);
            }
            auth.check(&user, password);
        }
        self.needs_redraw = true;
        true
    }

    /// The lock client's surface on `output`, while one is shown.
    pub fn lock_surface(
        &self,
        output: &Output,
    ) -> Option<&smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        self.locking
            .lock_surfaces
            .get(&output.name())
            .map(LockSurface::wl_surface)
    }

    /// The keyboard goes to the lock client's surface on the output under
    /// the pointer, or to nobody (our lock screen takes keys itself).
    fn lock_keyboard(&mut self) {
        let surface = self.client_lock().then(|| {
            let output = self.output_under_pointer()?;
            self.locking
                .lock_surfaces
                .get(&output.name())
                .or_else(|| self.locking.lock_surfaces.values().next())
                .map(|s| s.wl_surface().clone())
        });
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface.flatten(), SERIAL_COUNTER.next_serial());
        }
    }

    /// After each dispatch: holds, the curtain, the lock, a lock client
    /// gone, and the timer for what comes next.
    pub fn refresh_lock(&mut self) {
        let now = self.clock.now();
        // A crashed lock client leaves our lock screen, never an unlocked
        // session.
        if self.locked()
            && let By::Client(lock) = &self.locking.by
            && !lock.is_alive()
        {
            tracing::warn!("the lock client went away: our lock screen takes over");
            self.locking.by = By::Us;
            self.locking.lock_surfaces.clear();
            self.lock_keyboard();
            self.needs_redraw = true;
        }
        if self.idle_held() {
            self.locking.last_activity = now;
            if matches!(self.locking.phase, Phase::Curtain { .. }) {
                self.locking.phase = Phase::Unlocked;
                self.needs_redraw = true;
            }
        }
        let idle = self.config.idle;
        let delays = idle.delays(self.power_source());
        let idle_for = now.saturating_sub(self.locking.last_activity);
        match self.locking.phase {
            Phase::Unlocked
                if delays.curtain_after > 0
                    && idle_for >= Duration::from_secs(u64::from(delays.curtain_after)) =>
            {
                tracing::info!("idle: the curtain falls");
                self.locking.phase = Phase::Curtain { since: now };
                self.needs_redraw = true;
            }
            Phase::Curtain { since }
                if idle.lock
                    && now.saturating_sub(since)
                        >= Duration::from_secs(u64::from(idle.lock_after)) =>
            {
                self.lock_now();
            }
            _ => {}
        }
        if delays.sleep_after > 0
            && idle_for >= Duration::from_secs(u64::from(delays.sleep_after))
            && self.locking.slept_for != Some(self.locking.last_activity)
        {
            self.locking.slept_for = Some(self.locking.last_activity);
            // Hibernating only where the machine can, as the session menu.
            let power = match idle.sleep {
                hocus_core::config::Sleep::Hibernate if self.leaver.caps.hibernate => {
                    hocus_platform::power::Power::Hibernate
                }
                _ => hocus_platform::power::Power::Suspend,
            };
            tracing::info!(?power, "idle: the machine sleeps");
            self.sleep_now(power);
        }
        self.refresh_idle_notifications();
        self.tell_session();
        self.refresh_lock_panels();
        self.schedule_lock_timer();
    }

    /// Something keeps the session awake: an inhibitor's surface shown,
    /// or a fullscreen window.
    fn idle_held(&self) -> bool {
        let shown =
            |surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface| {
                self.window_for(surface)
                    .is_some_and(|w| self.space.element_location(&w).is_some())
            };
        self.protocols
            .inhibiting
            .iter()
            .any(|s| s.is_alive() && shown(s))
            || (self.config.idle.fullscreen_awake
                && self.space.outputs().any(|o| self.fullscreen_on(o)))
            || self.locking.dbus_inhibitors > 0
    }

    /// The next moment something happens without input: the curtain, the
    /// lock, a second of the countdown, an idle notification.
    fn next_deadline(&self) -> Option<Duration> {
        let delays = self.config.idle.delays(self.power_source());
        let mut next = match self.locking.phase {
            Phase::Unlocked if delays.curtain_after > 0 => Some(
                self.locking.last_activity + Duration::from_secs(u64::from(delays.curtain_after)),
            ),
            Phase::Curtain { since } => {
                let elapsed = self.clock.now().saturating_sub(since);
                // The countdown shows seconds.
                Some(since + Duration::from_secs(elapsed.as_secs() + 1))
            }
            _ => None,
        };
        let sleep = (delays.sleep_after > 0
            && self.locking.slept_for != Some(self.locking.last_activity))
        .then(|| self.locking.last_activity + Duration::from_secs(u64::from(delays.sleep_after)));
        for n in [self.next_idle_notification(), sleep].into_iter().flatten() {
            next = Some(next.map_or(n, |d| d.min(n)));
        }
        next
    }

    /// Wakes the loop at the next deadline (the virtual clock of tests
    /// moves by itself and needs none).
    fn schedule_lock_timer(&mut self) {
        let Some(handle) = self.waker().cloned() else {
            return;
        };
        let next = self.next_deadline();
        if self.locking.timer.as_ref().map(|t| t.1) == next {
            return;
        }
        if let Some((token, _)) = self.locking.timer.take() {
            handle.remove(token);
        }
        let Some(at) = next else {
            return;
        };
        let wait = at.saturating_sub(self.clock.now());
        match handle.insert_source(Timer::from_duration(wait), |_, _, app| {
            app.timed("idle deadline", |app| {
                app.state.locking.timer = None;
                app.state.needs_redraw = true;
            });
            TimeoutAction::Drop
        }) {
            Ok(token) => self.locking.timer = Some((token, at)),
            Err(err) => tracing::warn!(%err, "no idle timer"),
        }
    }

    /// The curtain's and the lock screens' contents, per output.
    fn refresh_lock_panels(&mut self) {
        self.refresh_lock_moments();
        // Unlocked and no curtain: nothing to draw, nor to work out (#88).
        let falling = matches!(self.locking.phase, Phase::Curtain { .. });
        if !falling && !(self.locked() && !self.client_lock()) {
            if !self.locking.curtains.is_empty() {
                self.locking.curtains.clear();
                self.needs_redraw = true;
            }
            return;
        }
        let now = self.clock.now();
        let theme = self.theme();
        let lang = self.shell_lang();
        let outputs: Vec<(String, UiSize, f32)> = self
            .space
            .outputs()
            .filter_map(|o| {
                let g = self.space.output_geometry(o)?;
                let scale = o.current_scale().fractional_scale() as f32;
                Some((
                    o.name(),
                    UiSize::new(g.size.w as f32, g.size.h as f32),
                    scale,
                ))
            })
            .collect();
        let curtain = match self.locking.phase {
            Phase::Curtain { since } => {
                let left = Duration::from_secs(u64::from(self.config.idle.lock_after))
                    .saturating_sub(now.saturating_sub(since));
                Some(left.as_secs_f64().ceil() as u32)
            }
            _ => None,
        };
        // The curtain's panda falls asleep from when it fell.
        let pandas = self.pandas_on();
        let sleepy = match self.locking.phase {
            Phase::Curtain { since } if pandas => Some(since),
            _ => None,
        };
        match curtain {
            Some(seconds) => {
                for (name, size, scale) in &outputs {
                    let panel = self
                        .locking
                        .curtains
                        .entry(name.clone())
                        .or_insert_with(|| {
                            Panel::new(Curtain::new(theme.clone(), lang, *size, *scale, sleepy))
                        });
                    panel.view.resize(*size, *scale);
                    panel.view.set_theme(theme.clone());
                    panel.view.set_seconds(seconds);
                    let drawn = panel.view.draw(now);
                    if panel.upload(&drawn) {
                        self.needs_redraw = true;
                    }
                }
            }
            None if !self.locking.curtains.is_empty() => {
                self.locking.curtains.clear();
                self.needs_redraw = true;
            }
            None => {}
        }
        let ours = self.locked() && !self.client_lock();
        if ours {
            let variant = if self.matrix_look() {
                Variant::Matrix
            } else {
                Variant::Plain
            };
            let user = self.user.clone();
            let time = self.wall_time();
            let caps = self
                .seat
                .get_keyboard()
                .is_some_and(|k| k.modifier_state().caps_lock);
            for (name, size, scale) in &outputs {
                let panel = self.locking.screens.entry(name.clone()).or_insert_with(|| {
                    Panel::new(LockScreen::new(
                        theme.clone(),
                        lang,
                        *size,
                        *scale,
                        &user,
                        pandas,
                        variant,
                    ))
                });
                panel.view.resize(*size, *scale);
                panel.view.set_theme(theme.clone());
                panel.view.set_time(time);
                panel.view.set_caps_lock(caps);
                let drawn = panel.view.draw(now);
                if panel.upload(&drawn) {
                    self.needs_redraw = true;
                }
            }
        }
    }

    /// The curtain over `output`, above everything but the cursor.
    pub fn curtain_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        self.locking
            .curtains
            .get(&output.name())
            .and_then(|p| p.element(renderer, output, Point::default()))
            .into_iter()
            .collect()
    }

    /// Everything a locked `output` shows: the lock client's surface, or
    /// our lock screen. Nothing else is drawn.
    pub fn lock_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> (
        Vec<WaylandSurfaceRenderElement<R>>,
        Vec<MemoryRenderBufferRenderElement<R>>,
    )
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = Scale::from(output.current_scale().fractional_scale());
        if self.client_lock()
            && let Some(surface) = self.locking.lock_surfaces.get(&output.name())
        {
            let elements = render_elements_from_surface_tree(
                renderer,
                surface.wl_surface(),
                Point::<i32, Physical>::default(),
                scale,
                1.0,
                Kind::Unspecified,
            );
            return (elements, Vec::new());
        }
        let now = self.clock.now();
        let alpha = self.locking.entering.as_ref().map(|m| match m {
            Moment::Plain(fade) => fade.value(now).clamp(0.0, 1.0),
            Moment::Matrix(since) => crate::matrix_screens::lock_shows(*since, now),
        });
        let Some(panel) = self.locking.screens.get(&output.name()) else {
            return (Vec::new(), Vec::new());
        };
        // Refused in the Matrix theme: the padlock and the field shake.
        let shaken = self
            .locking
            .refused
            .map(|since| crate::matrix_screens::shake(since, now))
            .filter(|dx| *dx != 0.0)
            .zip(panel.view.band());
        let ours = match shaken {
            Some((dx, band)) => {
                let size = self
                    .space
                    .output_geometry(output)
                    .map_or((band.w.round() as i32, 0), |g| (g.size.w, g.size.h));
                // Above the band, the band moved, below it; none less
                // than nothing on a screen too small for it.
                let top = (band.y.round() as i32).clamp(0, size.1);
                let h = (band.h.round() as i32).clamp(0, size.1 - top);
                let below = size.1 - top - h;
                [
                    (Rectangle::new((0, 0).into(), (size.0, top).into()), 0),
                    (
                        Rectangle::new((0, top).into(), (size.0, h).into()),
                        dx.round() as i32,
                    ),
                    (
                        Rectangle::new((0, top + h).into(), (size.0, below).into()),
                        0,
                    ),
                ]
                .into_iter()
                .filter_map(|(part, dx)| {
                    panel.element_part(
                        renderer,
                        output,
                        part.loc + Point::from((dx, 0)),
                        part,
                        alpha,
                    )
                })
                .collect()
            }
            None => panel
                .element_with(renderer, output, Point::default(), alpha)
                .into_iter()
                .collect(),
        };
        (Vec::new(), ours)
    }

    /// Our lock screen on `output`, for the Matrix theme's padlock and
    /// rain (#155).
    pub(crate) fn lock_screen_on(&self, output: &Output) -> Option<&Panel<LockScreen>> {
        if !self.locked() || self.client_lock() {
            return None;
        }
        self.locking.screens.get(&output.name())
    }

    /// Since when our lock screen comes in the Matrix theme, if it does.
    pub(crate) fn lock_matrix_since(&self) -> Option<Duration> {
        match self.locking.entering {
            Some(Moment::Matrix(since)) => Some(since),
            _ => None,
        }
    }

    /// Unlocked in the Matrix theme: since when, and the screen left.
    pub(crate) fn unlock_matrix_on(
        &self,
        output: &Output,
    ) -> Option<(Duration, &Panel<LockScreen>)> {
        match &self.locking.parting {
            Some((Moment::Matrix(since), screens)) => Some((*since, screens.get(&output.name())?)),
            _ => None,
        }
    }

    /// Whether the lock's moments move: not in tests that do not sample
    /// motion, nor with reduced motion.
    fn lock_moves(&self) -> bool {
        !self.instant_motion && !self.config.appearance.reduced_motion
    }

    /// Our lock screen still fading in: the desktop shows under it.
    pub fn lock_entering(&self) -> bool {
        let now = self.clock.now();
        self.locking.entering.as_ref().is_some_and(|m| match m {
            Moment::Plain(fade) => fade.is_running(now),
            // Until the rain has covered it.
            Moment::Matrix(since) => {
                now.saturating_sub(*since) < crate::matrix_screens::LOCK_COVERED
            }
        })
    }

    /// After each dispatch: frames while the lock fades in or parts, and
    /// the parted screens gone once open.
    fn refresh_lock_moments(&mut self) {
        let now = self.clock.now();
        use crate::matrix_screens::{LOCK_IN, REFUSED, UNLOCK};
        if let Some(moment) = &self.locking.entering {
            if !moment.is_running(now, LOCK_IN) {
                self.locking.entering = None;
            }
            self.needs_redraw = true;
        }
        if let Some((moment, _)) = &self.locking.parting {
            if !moment.is_running(now, UNLOCK) {
                self.locking.parting = None;
            }
            self.needs_redraw = true;
        }
        if let Some(since) = self.locking.refused {
            if now.saturating_sub(since) >= REFUSED {
                self.locking.refused = None;
            }
            self.needs_redraw = true;
        }
    }

    /// The unlocked desktop showing through our lock screen as its two
    /// halves part from the centre.
    pub fn parting_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some((Moment::Plain(open), screens)) = &self.locking.parting else {
            return Vec::new();
        };
        let (Some(panel), Some(g)) = (
            screens.get(&output.name()),
            self.space.output_geometry(output),
        ) else {
            return Vec::new();
        };
        let t = open.value(self.clock.now()).clamp(0.0, 1.0);
        let half = g.size.w / 2;
        let shift = (f64::from(half) * f64::from(t)).round() as i32;
        [
            (0, Point::from((-shift, 0))),
            (half, Point::from((half + shift, 0))),
        ]
        .into_iter()
        .filter_map(|(from, at)| {
            let part = smithay::utils::Rectangle::new((from, 0).into(), (half, g.size.h).into());
            // Opaque, as two doors: a half that faded would show what is
            // under it through the lock's own clock and texts.
            panel.element_part(renderer, output, at, part, None)
        })
        .collect()
    }

    /// The lock and the curtain for the control API.
    pub fn lock_ui(&self, out: &mut Vec<UiNode>) {
        for (name, panel) in self.locking.curtains.iter() {
            if let Some(o) = self
                .find_output(name)
                .and_then(|o| self.space.output_geometry(&o))
            {
                crate::shell::flatten(&panel.view.access(), o.loc, 0, out);
            }
        }
        for (name, panel) in self.locking.screens.iter() {
            if self.locked()
                && !self.client_lock()
                && let Some(o) = self
                    .find_output(name)
                    .and_then(|o| self.space.output_geometry(&o))
            {
                crate::shell::flatten(&panel.view.access(), o.loc, 0, out);
            }
        }
    }
}

impl SessionLockHandler for State {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.locking.state
    }

    /// A lock client may lock an unlocked session, or take over from our
    /// own lock screen; not from another living lock client.
    fn lock(&mut self, confirmation: SessionLocker) {
        if self.client_lock() {
            return;
        }
        let lock = confirmation.ext_session_lock().clone();
        self.locking.phase = Phase::Locked;
        self.locking.by = By::Client(lock);
        self.lock_began();
        // The next frame shows nothing but the lock.
        confirmation.lock();
    }

    fn unlock(&mut self) {
        if self.client_lock() {
            State::unlock(self);
        }
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        let Some(g) = self.space.output_geometry(&output) else {
            return;
        };
        surface.with_pending_state(|s| {
            s.size = Some(Size::from((g.size.w as u32, g.size.h as u32)));
        });
        surface.send_configure();
        self.locking.lock_surfaces.insert(output.name(), surface);
        self.lock_keyboard();
        self.needs_redraw = true;
    }
}
delegate_session_lock!(State);

impl State {
    /// Locked by a client: its surface under `pos`, where its origin is.
    pub fn lock_surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(
        smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        Point<f64, Logical>,
    )> {
        if !self.client_lock() {
            return None;
        }
        let output = self.space.output_under(pos).next()?;
        let origin = self.space.output_geometry(output)?.loc.to_f64();
        let surface = self.locking.lock_surfaces.get(&output.name())?;
        Some((surface.wl_surface().clone(), origin))
    }
}

/// logind and the ScreenSaver service: what the session asks of the lock.
pub fn start_session(
    handle: &LoopHandle<'static, App>,
    state: &mut State,
    session_bus: Option<&str>,
) {
    use hocus_platform::session::{Logind, Request, ScreenSaverService};
    let (requests, received) = channel::channel::<Request>();
    if let Err(err) = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(request) = event {
            app.timed("session", |app| app.state.session_request(request));
        }
    }) {
        tracing::warn!(err = %err.error, "the session's requests will not be heard");
        return;
    }
    // Only as the session itself: nested in a live session, logind's
    // inhibitors are the developer's machine's.
    if state.is_session || state.system_bus.is_some() {
        let tx = std::sync::Mutex::new(requests.clone());
        state.locking.logind = Logind::start(state.system_bus.clone(), move |r| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(r);
            }
        })
        .inspect_err(|err| tracing::warn!(%err, "no logind"))
        .ok();
    }
    // Only on a bus given: nested in a live session, its own screensaver
    // owns the name.
    if let Some(bus) = session_bus {
        let tx = std::sync::Mutex::new(requests);
        state.locking.screensaver = ScreenSaverService::start(bus, move |r| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(r);
            }
        })
        .inspect_err(|err| tracing::warn!(err, "no ScreenSaver service"))
        .ok();
    }
}

impl State {
    fn session_request(&mut self, request: hocus_platform::session::Request) {
        use hocus_platform::session::Request;
        match request {
            Request::Lock => self.lock_now(),
            // An administrator's unlock (loginctl unlock-session).
            Request::Unlock if self.locked() => State::unlock(self),
            Request::Unlock => {}
            Request::Activity => self.activity(),
            Request::Inhibitors(n) => self.locking.dbus_inhibitors = n,
            Request::Following => self.locking.logind_following += 1,
        }
    }

    /// Tells the session what shows: sleep may go on once locked, and the
    /// ScreenSaver service says whether the screen saver is active.
    fn tell_session(&mut self) {
        let active = self.locking.phase != Phase::Unlocked;
        if let Some(screensaver) = &self.locking.screensaver {
            screensaver.set_active(active);
        }
        let locked = self.locked();
        if locked
            && !self.locking.told_locked
            && let Some(logind) = &self.locking.logind
        {
            logind.locked();
        }
        self.locking.told_locked = locked;
    }
}
