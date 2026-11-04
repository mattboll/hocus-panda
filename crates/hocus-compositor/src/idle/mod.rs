//! When the session is idle (7.3, docs/design/idle.md): the countdown since
//! the user's last input, what holds it off, and the clients told.
//!
//! - **The countdown** (`[idle] delay`, off by default) runs from the
//!   user's last input. Reaching its end is [`Idle::Reached`], which the
//!   lock (7.2) will act on; the next input is [`Idle::Resumed`].
//! - **What holds it off**, each at once: a shown surface's
//!   `zwp_idle_inhibitor_v1`; an application's `org.freedesktop.ScreenSaver`
//!   `Inhibit`, until its `UnInhibit` or until it leaves the bus
//!   ([`screensaver`]); the focused window in full screen while it shows.
//!   Judged once a turn of the loop, without allocating. When the last
//!   hold lets go, the countdown starts again from zero. A hold keeps the
//!   session from becoming idle; it does not wake a session already idle
//!   (only the user does).
//! - **Clients' idle notifications** (`ext_idle_notifier_v1`, [`notify`]),
//!   each with its own timeout, held off the same way, except those made
//!   with `get_input_idle_notification`, which only input resets.
//!
//! One timer serves them all, armed for the nearest deadline. Input only
//! notes its time: the timer, firing before a deadline that input moved
//! on, sets itself again for the new one (`TimeoutAction::ToDuration`, the
//! same source). So input costs no timer, no allocation and no wakeup; a
//! deadline moved earlier (a hold letting go, a new notification) puts a
//! new one in its place; nothing to wait for, none is armed. Times are the
//! motions' clock (`State::now`), so that tests holding it step the
//! countdown too (`Headless::step`).

pub mod notify;
pub mod screensaver;

use std::{fmt, time::Duration};

use smithay::{
    desktop::{WindowSurfaceType, find_popup_root_surface, layer_map_for_output},
    reexports::{
        calloop::{
            RegistrationToken,
            channel::{self, Event},
            timer::{TimeoutAction, Timer},
        },
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    wayland::{
        compositor::get_parent,
        idle_inhibit::{IdleInhibitHandler, IdleInhibitManagerState},
        seat::WaylandFocus,
    },
};

use crate::{State, model::WindowId, shell::Bus};

/// What the session's idleness does, as it happens: the lock (7.2) hooks
/// into [`State::idle_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idle {
    /// No input for the whole delay, nothing holding it off.
    Reached,
    /// The user is back after it was reached.
    Resumed,
}

/// Where the session's countdown is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// No delay set: the session is never idle (clients' notifications
    /// still work).
    #[default]
    Off,
    /// Counting down since the last input, or since the last hold let go.
    Counting,
    /// Something holds it off.
    Held,
    /// Idle: no input since it was reached.
    Reached,
}

impl Phase {
    pub fn word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Counting => "counting",
            Self::Held => "held",
            Self::Reached => "idle",
        }
    }
}

/// What holds the session's idleness off, as `hocus msg idle` says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// A shown surface's idle inhibitor: its application (its window's
    /// app id, its layer's namespace, else its process).
    Inhibitor(String),
    /// An application's `org.freedesktop.ScreenSaver` `Inhibit`.
    ScreenSaver { app: String, reason: String },
    /// The focused window, in full screen: its app id.
    Fullscreen(String),
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inhibitor(app) => write!(f, "inhibitor {app}"),
            Self::ScreenSaver { app, reason } => write!(f, "screensaver {app}: {reason}"),
            Self::Fullscreen(app) => write!(f, "fullscreen {app}"),
        }
    }
}

/// The session's idleness as of now: `hocus msg idle`'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub phase: Phase,
    pub delay: Option<Duration>,
    /// While counting: how long until idle.
    pub left: Option<Duration>,
    pub holders: Vec<Holder>,
}

/// One `key value` a line, a line per holder.
impl fmt::Display for Said {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "state {}", self.phase.word())?;
        match self.delay {
            Some(delay) => writeln!(f, "delay {} s", delay.as_secs())?,
            None => writeln!(f, "delay off")?,
        }
        if let Some(left) = self.left {
            writeln!(f, "left {:.1} s", left.as_secs_f64())?;
        }
        if self.holders.is_empty() {
            writeln!(f, "held-by nothing")?;
        }
        for holder in &self.holders {
            writeln!(f, "held-by {}", text_line(&holder.to_string()))?;
        }
        Ok(())
    }
}

/// What a client said, on one line.
fn text_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The idle timer's work so far, for tests: whether one is armed, how many
/// were put in, how often one fired, and how often idle was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub armed: bool,
    pub timers: u64,
    pub wakeups: u64,
    pub reached: u64,
    /// The clients' notifications.
    pub notifications: usize,
}

/// The session's idleness and the clients' notifications.
#[derive(Debug)]
pub struct Idleness {
    delay: Option<Duration>,
    phase: Phase,
    /// The user's last input.
    input_at: Duration,
    /// Where the countdown runs from: the last activity, or the moment the
    /// last hold let go.
    counting_from: Duration,
    /// Something holds it off, as judged last.
    held: bool,
    /// The timer, and the deadline it is set for.
    timer: Option<(Duration, RegistrationToken)>,
    /// The surfaces with an idle inhibitor, shown or not.
    inhibitors: Vec<WlSurface>,
    /// The applications' `Inhibit`s, as the bus's thread last said.
    screensaver: Vec<screensaver::Hold>,
    notifications: Vec<notify::Notification>,
    counts: Counts,
    _manager: IdleInhibitManagerState,
}

impl Idleness {
    pub fn new(
        dh: &smithay::reexports::wayland_server::DisplayHandle,
        delay: Option<Duration>,
    ) -> Self {
        notify::global(dh);
        Self {
            delay,
            phase: Phase::Off,
            input_at: Duration::ZERO,
            counting_from: Duration::ZERO,
            held: false,
            timer: None,
            inhibitors: Vec::new(),
            screensaver: Vec::new(),
            notifications: Vec::new(),
            counts: Counts::default(),
            _manager: IdleInhibitManagerState::new::<State>(dh),
        }
    }

    /// The nearest deadline: the session's, while it counts down, and each
    /// notification's not idle yet and not held off.
    fn next(&self) -> Option<Duration> {
        let mut next: Option<Duration> = None;
        let mut take = |at: Duration| next = Some(next.map_or(at, |n| n.min(at)));
        if self.phase == Phase::Counting
            && let Some(delay) = self.delay
        {
            take(self.counting_from + delay);
        }
        for n in &self.notifications {
            if !n.idled && (n.input || !self.held) {
                take(n.deadline(self.input_at, self.counting_from));
            }
        }
        next
    }
}

impl State {
    /// As the loop starts: the countdown from now, and the applications'
    /// `Inhibit`s served on the session bus, if there is one.
    pub fn start_idle(&mut self) {
        let now = self.now();
        self.idle.input_at = now;
        self.idle.counting_from = now;
        self.idle.phase = match self.idle.delay {
            Some(_) => Phase::Counting,
            None => Phase::Off,
        };
        tracing::info!(delay = ?self.idle.delay, "idle");
        self.idle_arm();
        let Some(bus) = self.shell.bus.clone() else {
            tracing::info!("no session bus: no org.freedesktop.ScreenSaver");
            return;
        };
        let word = match bus {
            Bus::Given(address) => address,
            Bus::Session => "session".to_owned(),
        };
        let (sender, receiver) = channel::channel::<screensaver::FromBus>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.screensaver_said(said);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "could not listen to org.freedesktop.ScreenSaver");
            return;
        }
        screensaver::start(word, sender);
    }

    fn screensaver_said(&mut self, said: screensaver::FromBus) {
        match said {
            // Judged with the rest after this turn ([`State::flush_idle`]).
            screensaver::FromBus::Holds(holds) => self.idle.screensaver = holds,
            screensaver::FromBus::Activity => self.idle_active(false),
        }
    }

    /// The user's input, from a device or a test.
    pub fn idle_input(&mut self) {
        self.idle_active(true);
    }

    /// Activity: the user's input, or an application saying the user is
    /// there (`SimulateUserActivity`), which only the inhibitors' honouring
    /// notifications take. Every deadline moves on; the timer set for an
    /// earlier one finds it moved as it fires.
    fn idle_active(&mut self, input: bool) {
        let now = self.now();
        let idle = &mut self.idle;
        if input {
            idle.input_at = now;
        }
        idle.counting_from = now;
        let resumed = idle.phase == Phase::Reached;
        if resumed {
            idle.phase = if idle.held {
                Phase::Held
            } else {
                Phase::Counting
            };
        }
        for n in idle.notifications.iter_mut().filter(|n| n.idled) {
            if input || !n.input {
                n.resume();
            }
        }
        if resumed {
            self.idle_event(Idle::Resumed);
        }
        if self.idle.timer.is_none() {
            self.idle_arm();
        }
    }

    /// Once a turn of the loop: whether something holds idleness off now.
    /// When the last hold lets go, the countdown starts again from zero.
    pub fn flush_idle(&mut self) {
        if self.idle.inhibitors.iter().any(|s| !s.is_alive()) {
            self.idle.inhibitors.retain(Resource::is_alive);
        }
        let held = self.idle_held();
        if held == self.idle.held {
            return;
        }
        let idle = &mut self.idle;
        idle.held = held;
        match (held, idle.phase) {
            (true, Phase::Counting) => idle.phase = Phase::Held,
            (false, Phase::Held) => idle.phase = Phase::Counting,
            _ => {}
        }
        if held {
            tracing::info!(holders = ?self.idle_holders(), "idle held off");
        } else {
            self.idle.counting_from = self.now();
            tracing::info!("idle no longer held off: the countdown starts again");
        }
        self.idle_arm();
    }

    /// Something holds idleness off: an `Inhibit`, the focused window in
    /// full screen, a shown surface's inhibitor.
    fn idle_held(&self) -> bool {
        !self.idle.screensaver.is_empty()
            || self.fullscreen_holding().is_some()
            || self.idle.inhibitors.iter().any(|s| self.inhibitor_shown(s))
    }

    /// The focused window, if it is in full screen and shows.
    fn fullscreen_holding(&self) -> Option<WindowId> {
        let id = self.model.focused()?;
        (self.model.mode(id) == crate::model::Mode::Fullscreen && self.model.is_shown(id))
            .then_some(id)
    }

    /// Whether an inhibitor's surface shows: its window is not minimised
    /// and is on its screen's current desktop (a subsurface or a popup,
    /// its window's), or it is a layer surface on a screen.
    fn inhibitor_shown(&self, surface: &WlSurface) -> bool {
        let root = self.root_surface(surface);
        if let Some(id) = self.window_of_surface(&root) {
            return self.model.is_shown(id);
        }
        self.space.outputs().any(|o| {
            layer_map_for_output(o)
                .layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                .is_some()
        })
    }

    /// The surface at the root of `surface`'s subsurfaces and popups.
    fn root_surface(&self, surface: &WlSurface) -> WlSurface {
        let mut root = surface.clone();
        // A popup's parent may be a subsurface, of a popup...: a few
        // levels at most, bounded all the same.
        for _ in 0..16 {
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            match self
                .popups
                .find_popup(&root)
                .and_then(|p| find_popup_root_surface(&p).ok())
            {
                Some(parent) if parent != root => root = parent,
                _ => break,
            }
        }
        root
    }

    fn window_of_surface(&self, surface: &WlSurface) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, m)| m.window.wl_surface().is_some_and(|s| *s == *surface))
            .map(|(id, _)| *id)
    }

    /// What holds idleness off now, for `hocus msg idle` and the log.
    pub fn idle_holders(&self) -> Vec<Holder> {
        let mut holders = Vec::new();
        for surface in &self.idle.inhibitors {
            if surface.is_alive() && self.inhibitor_shown(surface) {
                holders.push(Holder::Inhibitor(self.surface_app(surface)));
            }
        }
        for hold in &self.idle.screensaver {
            holders.push(Holder::ScreenSaver {
                app: hold.app.clone(),
                reason: hold.reason.clone(),
            });
        }
        if let Some(id) = self.fullscreen_holding() {
            let app = self
                .windows
                .get(&id)
                .and_then(|m| crate::windows::app_id(&m.window))
                .unwrap_or_default();
            holders.push(Holder::Fullscreen(app));
        }
        holders
    }

    /// A surface's application, as a person reads it.
    fn surface_app(&self, surface: &WlSurface) -> String {
        let root = self.root_surface(surface);
        let window = self
            .window_of_surface(&root)
            .and_then(|id| self.windows.get(&id))
            .and_then(|m| crate::windows::app_id(&m.window));
        if let Some(app) = window {
            return app;
        }
        let layer = self.space.outputs().find_map(|o| {
            layer_map_for_output(o)
                .layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                .map(|l| l.namespace().to_owned())
        });
        if let Some(namespace) = layer {
            return namespace;
        }
        self.client_pid(surface)
            .map_or_else(|| "a surface".to_owned(), |pid| format!("pid {pid}"))
    }

    /// The session's idleness now.
    pub fn idle_said(&self) -> Said {
        let left = match (self.idle.phase, self.idle.delay) {
            (Phase::Counting, Some(delay)) => {
                Some((self.idle.counting_from + delay).saturating_sub(self.now()))
            }
            _ => None,
        };
        Said {
            phase: self.idle.phase,
            delay: self.idle.delay,
            left,
            holders: self.idle_holders(),
        }
    }

    pub fn idle_counts(&self) -> Counts {
        Counts {
            armed: self.idle.timer.is_some(),
            notifications: self.idle.notifications.len(),
            ..self.idle.counts
        }
    }

    /// What is due now: the session reaching idle, notifications becoming
    /// idle. The next deadline after, if any.
    fn idle_due(&mut self) -> Option<Duration> {
        let now = self.now();
        let idle = &mut self.idle;
        let reached = idle.phase == Phase::Counting
            && idle
                .delay
                .is_some_and(|delay| now >= idle.counting_from + delay);
        if reached {
            idle.phase = Phase::Reached;
            idle.counts.reached += 1;
        }
        let (input_at, counting_from, held) = (idle.input_at, idle.counting_from, idle.held);
        for n in &mut idle.notifications {
            if !n.idled && (n.input || !held) && now >= n.deadline(input_at, counting_from) {
                n.idle();
            }
        }
        if reached {
            self.idle_event(Idle::Reached);
        }
        self.idle.next()
    }

    /// What is due now, and the timer for what comes next: the held clock
    /// stepped (`Headless::step`), a notification made.
    pub fn idle_check(&mut self) {
        self.idle_due();
        self.idle_arm();
    }

    /// The timer for the nearest deadline: kept when it fires first (it
    /// sets itself again then), replaced when the deadline is earlier,
    /// removed when there is none.
    fn idle_arm(&mut self) {
        let next = self.idle.next();
        match (self.idle.timer.as_ref().map(|(at, _)| *at), next) {
            (Some(at), Some(next)) if at <= next => return,
            (None, None) => return,
            _ => {}
        }
        if let Some((_, token)) = self.idle.timer.take() {
            self.loop_handle.remove(token);
        }
        let Some(at) = next else {
            return;
        };
        let timer = Timer::from_duration(at.saturating_sub(self.now()));
        let inserted = self
            .loop_handle
            .insert_source(timer, |_, _, app| app.state.idle_timer_fired());
        match inserted {
            Ok(token) => {
                self.idle.timer = Some((at, token));
                self.idle.counts.timers += 1;
            }
            Err(err) => tracing::warn!(%err.error, "no idle timer"),
        }
    }

    fn idle_timer_fired(&mut self) -> TimeoutAction {
        self.idle.counts.wakeups += 1;
        match self.idle_due() {
            Some(at) => {
                if let Some((set, _)) = &mut self.idle.timer {
                    *set = at;
                }
                TimeoutAction::ToDuration(at.saturating_sub(self.now()))
            }
            None => {
                self.idle.timer = None;
                TimeoutAction::Drop
            }
        }
    }

    /// The session became idle, or the user came back: where the lock
    /// (7.2) will act. For now it is only logged.
    pub fn idle_event(&mut self, event: Idle) {
        match event {
            Idle::Reached => tracing::info!(delay = ?self.idle.delay, "idle reached"),
            Idle::Resumed => tracing::info!("idle no more: the user is back"),
        }
    }
}

impl IdleInhibitHandler for State {
    fn inhibit(&mut self, surface: WlSurface) {
        tracing::debug!(app = self.surface_app(&surface), "an idle inhibitor");
        self.idle.inhibitors.push(surface);
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        // A surface may have several inhibitors: one goes.
        if let Some(at) = self.idle.inhibitors.iter().position(|s| *s == surface) {
            self.idle.inhibitors.remove(at);
        }
    }
}

smithay::delegate_idle_inhibit!(State);
