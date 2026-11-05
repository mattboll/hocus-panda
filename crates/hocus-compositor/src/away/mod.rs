//! Locking on idle and before sleep (7.2, docs/design/idle.md and
//! docs/design/lock.md): the curtain, then the lock, then the screens off.
//!
//! - **The curtain.** When the session is idle ([`crate::idle`]'s
//!   `Idle::Reached`, its delay the power source's: `[idle] delay` on AC,
//!   `[power.battery] screen-off` on battery), every screen dims over
//!   [`hocus_link::CURTAIN`]; on the screen under the pointer the shell's
//!   pill says the session is about to lock. Any input lifts it, and is
//!   swallowed: a move, a click or a key that lifts it does nothing else.
//! - **The lock** comes as the curtain ends, fading in over its last look
//!   (`[power.*] lock-after = "0s"`, the default). With a delay there, the
//!   screens go off as the curtain ends, and the session locks that long
//!   after; with `"never"`, the screens only go off.
//! - **The screens off**, [`SCREEN_OFF_LOCKED`] after the lock (or the
//!   last input while locked): DPMS through the DRM backend; black and
//!   drawing nothing headless. Input turns them on again, still locked.
//!   The lid closed turns the laptop's own screens off; it never locks:
//!   logind sleeps, or not (docked, `HandleLidSwitchDocked`), and the lock
//!   before sleep is what locks then.
//! - **Before sleep** ([`system`]): logind's delay inhibitor held for the
//!   whole session; at `PrepareForSleep(true)` the lock is up at once,
//!   without its fade, and the inhibitor goes once every screen that is on
//!   presented a frame of it, or after [`SLEEP_WAIT`] at most.
//!
//! One timer, on the motions' clock (`State::now`), armed for the nearest
//! deadline: the curtain's end, the lock after the screens went off, the
//! screens off while locked, the sleep's wait. Input moves the last one
//! on without touching the timer, which finds it moved as it fires (as the
//! idle countdown's). At rest nothing is armed but that.

pub mod draw;
pub mod system;

use std::{collections::HashSet, os::fd::OwnedFd, time::Duration};

use hocus_link::{CURTAIN, CURTAIN_NAMESPACE, CurtainShown, PILL_TOP, ToShell};
use smithay::{
    backend::renderer::element::Id,
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        channel::{self, Event},
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point},
};

use crate::{State, decoration::FrameIds};

/// The screens go off this long after the lock, or after the last input
/// while locked.
pub const SCREEN_OFF_LOCKED: Duration = Duration::from_secs(30);
/// The machine sleeps once the lock shows, or after this at most.
pub const SLEEP_WAIT: Duration = Duration::from_secs(2);
/// The pointer moving less than this, all told since the last input that
/// counted, is not the user's (a mouse on a shaking desk).
const DRIFT: f64 = 4.0;

/// Where the session is, away from its user.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// The user is here, or idle has not come, or the session is locked.
    Here,
    /// The curtain dims since `since`; as it ends, the session locks
    /// (`locks`) or the screens go off. The pill on screen `output`, from
    /// its first frame (`pill_at`).
    Curtain {
        since: Duration,
        locks: bool,
        output: String,
        pill_at: Option<Duration>,
    },
    /// The screens off, unlocked (a lock delay, or none); the session
    /// locks at `lock_at`.
    Dark { lock_at: Option<Duration> },
}

/// The curtain lifted, fading out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lifted {
    pub at: Duration,
    /// The dim's and the pill's look when it was lifted.
    pub dim: f32,
    pub pill: draw::PillLook,
    /// The pill's screen, if it had one.
    pub pill_output: Option<String>,
}

/// The curtain ended in the lock: its look at its end, which the lock's
/// picture of the desktop is taken with (it fades in over it).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ended {
    pub pill_output: Option<String>,
}

/// What was pressed while the curtain swallowed it: its release is
/// swallowed too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pressed {
    Key(u32),
    Button(u32),
    Gesture,
}

/// The user's input, as [`State::felt`] judges it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Felt {
    Press(Pressed),
    Release(Pressed),
    /// The pointer moved by this much.
    Motion(Point<f64, Logical>),
    /// A gesture going on.
    GestureUpdate,
    /// Anything else: a wheel, a lid opened.
    Other,
}

/// What [`State::away`] says, for tests and `hocus msg idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Here,
    /// The curtain is up: it ends in the lock (`locks`) or the screens
    /// off.
    Curtain {
        locks: bool,
    },
    /// The screens are off, unlocked.
    Dark,
}

/// Counts for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub armed: bool,
    pub timers: u64,
    pub wakeups: u64,
    pub curtains: u64,
    /// Inputs swallowed (each press, motion, wheel).
    pub swallowed: u64,
    /// Sleeps let go, and of those after the wait ran out.
    pub slept: u64,
    pub slept_late: u64,
}

/// The sleep's side: logind's inhibitor, and the wait before letting go.
#[derive(Debug, Default)]
pub(crate) struct Sleep {
    /// logind's delay inhibitor (its file descriptor): held, the machine
    /// waits for us before it sleeps.
    pub inhibitor: Option<OwnedFd>,
    /// `PrepareForSleep(true)` came: the inhibitor goes once the lock
    /// shows, or at this time at most.
    pub until: Option<Duration>,
    /// What asks the system bus's thread for a new inhibitor.
    pub to_system: Option<tokio::sync::mpsc::UnboundedSender<system::ToSystem>>,
}

/// Locking on idle and before sleep, owned by the loop.
#[derive(Debug)]
pub struct Away {
    phase: Phase,
    pub(crate) lifted: Option<Lifted>,
    /// Ended in the lock: its look at the end, for the lock's picture of
    /// the desktop to fade in over.
    pub(crate) under_lock: Option<Ended>,
    /// Every screen off (idle).
    screens_off: bool,
    /// The lid closed: the laptop's own screens off.
    lid_closed: bool,
    /// On battery, as UPower says (on AC until it says).
    battery: bool,
    /// The pointer's motion since the last input that counted.
    drift: Point<f64, Logical>,
    swallowed: HashSet<Pressed>,
    timer: Option<(Duration, RegistrationToken)>,
    pub(crate) sleep: Sleep,
    /// The machine woke up: what the screens showed may be gone (a
    /// display engine reset by the sleep), the backend draws them afresh.
    pub woke: bool,
    pub(crate) dim: Id,
    pub(crate) shadow: FrameIds,
    counts: Counts,
}

impl Default for Away {
    fn default() -> Self {
        Self {
            phase: Phase::Here,
            lifted: None,
            under_lock: None,
            screens_off: false,
            lid_closed: false,
            battery: false,
            drift: Point::default(),
            swallowed: HashSet::new(),
            timer: None,
            sleep: Sleep::default(),
            woke: false,
            dim: Id::new(),
            shadow: FrameIds::default(),
            counts: Counts::default(),
        }
    }
}

/// A laptop's own screen, by its connector's name: the lid closes over
/// it. A headless output is one when a test says so.
#[derive(Debug, Clone, Copy)]
pub struct Internal;

pub fn is_internal(output: &Output) -> bool {
    let name = output.name();
    ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p))
        || output.user_data().get::<Internal>().is_some()
}

impl State {
    /// As the loop starts: the system bus read on its thread, if there is
    /// one (logind's sleeps, UPower's power source).
    pub fn start_away(&mut self, bus: Option<system::Bus>) {
        let Some(bus) = bus else {
            tracing::info!("no system bus: no lock before sleep, on AC as far as idle goes");
            return;
        };
        let (sender, receiver) = channel::channel::<system::FromSystem>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.system_said(said);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "could not listen to the system bus");
            return;
        }
        let (to, asks) = tokio::sync::mpsc::unbounded_channel();
        self.away.sleep.to_system = Some(to);
        system::start(bus, sender, asks);
    }

    fn system_said(&mut self, said: system::FromSystem) {
        match said {
            system::FromSystem::Inhibitor(fd) => self.away_inhibitor(fd),
            system::FromSystem::PrepareForSleep(sleeping) => self.prepare_for_sleep(sleeping),
            system::FromSystem::OnBattery(battery) => self.away_on_battery(battery),
        }
    }

    // --- What it says ---------------------------------------------------------------

    /// Where the session is, away from its user.
    pub fn away(&self) -> Shown {
        match self.away.phase {
            Phase::Here => Shown::Here,
            Phase::Curtain { locks, .. } => Shown::Curtain { locks },
            Phase::Dark { .. } => Shown::Dark,
        }
    }

    /// Whether `output` is on: off while idle turned every screen off, or
    /// the lid closed over it.
    pub fn output_powered(&self, output: &Output) -> bool {
        !(self.away.screens_off
            || (self.away.lid_closed && is_internal(output))
            || self.display_off(output))
    }

    /// Whether the lid is closed.
    pub fn lid_closed(&self) -> bool {
        self.away.lid_closed
    }

    /// Whether every screen is off (idle).
    pub fn screens_off(&self) -> bool {
        self.away.screens_off
    }

    pub fn on_battery(&self) -> bool {
        self.away.battery
    }

    /// Whether logind's sleep inhibitor is held now.
    pub fn sleep_inhibited(&self) -> bool {
        self.away.sleep.inhibitor.is_some()
    }

    pub fn away_counts(&self) -> Counts {
        Counts {
            armed: self.away.timer.is_some(),
            ..self.away.counts
        }
    }

    /// The curtain's start, while it is up.
    pub(crate) fn curtain_since(&self) -> Option<Duration> {
        match self.away.phase {
            Phase::Curtain { since, .. } => Some(since),
            _ => None,
        }
    }

    /// The pill's screen and its first frame, while the curtain is up and
    /// ends in the lock.
    pub(crate) fn curtain_pill(&self) -> Option<(&str, Option<Duration>)> {
        match &self.away.phase {
            Phase::Curtain {
                locks: true,
                output,
                pill_at,
                ..
            } => Some((output.as_str(), *pill_at)),
            _ => None,
        }
    }

    /// `away`'s lines for `hocus msg idle`.
    pub fn away_said(&self) -> String {
        let away = match self.away() {
            Shown::Here => "here",
            Shown::Curtain { locks: true } => "curtain locking",
            Shown::Curtain { locks: false } => "curtain",
            Shown::Dark => "dark",
        };
        let off: Vec<String> = self
            .space
            .outputs()
            .filter(|o| !self.output_powered(o))
            .map(Output::name)
            .collect();
        let screens = if off.is_empty() {
            "on".to_owned()
        } else {
            format!("off {}", off.join(" "))
        };
        format!(
            "away {away}\nscreens {screens}\npower {}\nsleep-inhibitor {}\n",
            if self.away.battery { "battery" } else { "ac" },
            if self.sleep_inhibited() {
                "held"
            } else {
                "none"
            },
        )
    }

    // --- Idle, and the curtain ----------------------------------------------------

    /// Idle is reached: the curtain comes, unless locked (the screens go
    /// off on their own deadline then).
    pub(crate) fn away_idle(&mut self) {
        if self.locked() || self.away.phase != Phase::Here {
            return;
        }
        let now = self.now();
        let locks = self.config.lock_after(self.away.battery) == Some(Duration::ZERO);
        let output = self
            .output_under_pointer()
            .map(|o| o.name())
            .unwrap_or_default();
        tracing::info!(locks, output, "the curtain comes");
        self.away.counts.curtains += 1;
        self.lifted_gone();
        self.away.phase = Phase::Curtain {
            since: now,
            locks,
            output,
            pill_at: None,
        };
        self.tell_curtain();
        self.redraw = true;
        self.away_arm();
    }

    /// The shell is told of the pill: only a curtain that ends in the lock
    /// has one ("Verrouillage dans 7 s").
    fn tell_curtain(&mut self) {
        let Phase::Curtain {
            since,
            locks: true,
            output,
            ..
        } = &self.away.phase
        else {
            return;
        };
        let Some(screen) = self.space.outputs().find(|o| o.name() == *output) else {
            return;
        };
        let height = self.space.output_geometry(screen).map_or(0, |g| g.size.h);
        let elapsed = self.now().saturating_sub(*since);
        let shown = CurtainShown {
            elapsed: u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX),
            length: CURTAIN.as_millis() as u32,
            top: (f64::from(height) * PILL_TOP).round() as i32,
            motion: !self.reduced_motion(),
            output: output.clone(),
        };
        self.tell_shell(ToShell::Curtain(shown));
    }

    /// The shell connected: it is told the pill again.
    pub(crate) fn away_shell_connected(&mut self) {
        self.tell_curtain();
    }

    /// The shell's pill surface on `output`, if the pill is there: of its
    /// own connection only.
    pub(crate) fn curtain_layer(&self, output: &Output, pill: &str) -> Option<LayerSurface> {
        if pill != output.name() {
            return None;
        }
        let map = layer_map_for_output(output);
        map.layers()
            .find(|l| l.namespace() == CURTAIN_NAMESPACE && self.is_shell_surface(l.wl_surface()))
            .cloned()
    }

    /// A commit of the pill's surface: its first frame starts its motion.
    pub(crate) fn curtain_committed(&mut self, layer: &LayerSurface, output: &Output) {
        if !self.is_shell_surface(layer.wl_surface()) {
            tracing::warn!("a curtain surface from another client than the shell: never shown");
            return;
        }
        let drawn = smithay::backend::renderer::utils::with_renderer_surface_state(
            layer.wl_surface(),
            |s| s.buffer().is_some(),
        )
        .unwrap_or(false);
        let now = self.now();
        if let Phase::Curtain {
            output: on,
            pill_at,
            ..
        } = &mut self.away.phase
            && drawn
            && *on == output.name()
        {
            pill_at.get_or_insert(now);
        }
    }

    /// The curtain goes: lifted by the user (fading out), or ended.
    fn curtain_down(&mut self, lifted: bool) {
        let Phase::Curtain {
            since,
            locks,
            output,
            pill_at,
        } = self.away.phase.clone()
        else {
            return;
        };
        let now = self.now();
        if lifted {
            let (dim, pill) = self.curtain_looks(since, pill_at, now);
            self.away.lifted = Some(Lifted {
                at: now,
                dim,
                pill,
                pill_output: locks.then_some(output),
            });
        }
        self.away.phase = Phase::Here;
        // Lifted, the pill fades out first ([`State::lifted_gone`]).
        if locks && !lifted {
            self.tell_shell(ToShell::CurtainEnd);
        }
        self.redraw = true;
    }

    // --- Input --------------------------------------------------------------------

    /// The user's input as it comes, before it goes anywhere: activity
    /// (7.3), which lifts the curtain and turns the screens on (7.2).
    /// Whether it goes on: what lifts the curtain, or wakes the screens
    /// of a session not locked, is swallowed, its release too.
    pub fn felt(&mut self, felt: Felt) -> bool {
        match felt {
            Felt::Release(pressed) if self.away.swallowed.remove(&pressed) => return false,
            Felt::GestureUpdate if self.away.swallowed.contains(&Pressed::Gesture) => {
                return false;
            }
            _ => {}
        }
        let swallows = self.away_swallows();
        if let Felt::Motion(by) = felt {
            let drift = self.away.drift + by;
            if drift.x.hypot(drift.y) < DRIFT {
                self.away.drift = drift;
                if swallows {
                    self.away.counts.swallowed += 1;
                }
                return !swallows;
            }
        }
        self.away.drift = Point::default();
        if swallows {
            self.away.counts.swallowed += 1;
            if let Felt::Press(pressed) = felt {
                self.away.swallowed.insert(pressed);
            }
        }
        self.idle_input();
        self.away_input();
        !swallows
    }

    /// Whether input now is swallowed: the curtain up, or the screens off
    /// unlocked. Locked, it goes to the lock (its field).
    fn away_swallows(&self) -> bool {
        !self.locked()
            && (matches!(self.away.phase, Phase::Curtain { .. } | Phase::Dark { .. })
                || self.away.screens_off)
    }

    /// The user is here: the curtain lifts, the screens come on.
    fn away_input(&mut self) {
        match self.away.phase {
            Phase::Curtain { .. } => {
                tracing::info!("the curtain lifts: the user is back");
                self.lifted_gone();
                self.curtain_down(true);
            }
            Phase::Dark { .. } => self.away.phase = Phase::Here,
            Phase::Here => {}
        }
        if self.away.screens_off {
            tracing::info!("the screens come on");
            self.away.screens_off = false;
            self.redraw = true;
        }
        self.away.under_lock = None;
        self.away_arm();
    }

    /// The lid closed or opened: the laptop's own screens off or on;
    /// opened, the user is there.
    pub fn lid(&mut self, closed: bool) {
        if self.away.lid_closed == closed {
            return;
        }
        tracing::info!(closed, "the lid");
        self.away.lid_closed = closed;
        self.displays.dirty = true;
        self.redraw = true;
        if !closed {
            self.felt(Felt::Other);
        }
    }

    /// UPower says the machine is on battery, or on AC: the idle delay is
    /// that source's.
    pub(crate) fn away_on_battery(&mut self, battery: bool) {
        if self.away.battery == battery {
            return;
        }
        tracing::info!(battery, "the power source");
        self.away.battery = battery;
        let delay = self.config.idle_delay(battery);
        self.idle_set_delay(delay);
    }

    // --- The lock -----------------------------------------------------------------

    /// The session locked (by anything): its screens go off in a while.
    pub(crate) fn away_locked(&mut self) {
        if let Phase::Curtain { .. } | Phase::Dark { .. } = self.away.phase
            && self.away.under_lock.is_none()
        {
            self.curtain_down(false);
            self.away.phase = Phase::Here;
        }
        self.away_arm();
    }

    /// The session opened: nothing of the curtain stays.
    pub(crate) fn away_unlocked(&mut self) {
        self.away.under_lock = None;
        self.lifted_gone();
        self.away_arm();
    }

    // --- Before sleep --------------------------------------------------------------

    /// logind says the machine is about to sleep (`true`), or woke up.
    pub(crate) fn prepare_for_sleep(&mut self, sleeping: bool) {
        if sleeping {
            tracing::info!("the machine is about to sleep: the session locks");
            self.curtain_down(false);
            self.away.phase = Phase::Here;
            self.lifted_gone();
            if !self.locked() {
                self.lock_at_once();
            }
            self.away.sleep.until = Some(self.now() + SLEEP_WAIT);
            self.away_after_turn();
            self.away_arm();
        } else {
            tracing::info!("the machine woke up: locked, the field holds the keyboard");
            self.away.sleep.until = None;
            if self.away.sleep.inhibitor.is_none() {
                self.ask_inhibitor();
            }
            self.lock_woke();
            self.away.woke = true;
            self.redraw = true;
            // The user woke it: the screens on, the countdowns from now.
            self.felt(Felt::Other);
        }
    }

    /// logind's inhibitor came: held until the next sleep.
    pub(crate) fn away_inhibitor(&mut self, fd: OwnedFd) {
        tracing::info!("logind's sleep inhibitor held: the session locks before sleeping");
        self.away.sleep.inhibitor = Some(fd);
    }

    fn ask_inhibitor(&mut self) {
        if let Some(to) = &self.away.sleep.to_system {
            let _ = to.send(system::ToSystem::Inhibit);
        }
    }

    /// After each turn: once the lock shows on every screen that is on,
    /// the machine may sleep.
    pub(crate) fn away_after_turn(&mut self) {
        if self.away.sleep.until.is_some() && self.lock_presented() {
            self.let_sleep(false);
        }
    }

    /// The inhibitor goes: logind puts the machine to sleep.
    fn let_sleep(&mut self, late: bool) {
        self.away.sleep.until = None;
        self.away.counts.slept += 1;
        if late {
            self.away.counts.slept_late += 1;
            tracing::warn!("the lock did not show in time: the machine sleeps all the same");
        } else {
            tracing::info!("the lock shows on every screen: the machine may sleep");
        }
        // Closing it is what lets logind go on.
        drop(self.away.sleep.inhibitor.take());
    }

    // --- The timer -----------------------------------------------------------------

    /// The nearest deadline.
    fn away_next(&self) -> Option<Duration> {
        let mut next: Option<Duration> = None;
        let mut take = |at: Duration| next = Some(next.map_or(at, |n| n.min(at)));
        match self.away.phase {
            Phase::Curtain { since, .. } => take(since + CURTAIN),
            Phase::Dark { lock_at: Some(at) } => take(at),
            _ => {}
        }
        if !self.away.screens_off
            && let Some(since) = self.lock_since()
        {
            take(since.max(self.idle_input_at()) + SCREEN_OFF_LOCKED);
        }
        if let Some(until) = self.away.sleep.until {
            take(until);
        }
        next
    }

    /// What is due now; the next deadline after.
    fn away_due(&mut self) -> Option<Duration> {
        let now = self.now();
        if let Phase::Curtain {
            since,
            locks,
            ref output,
            ..
        } = self.away.phase
            && now >= since + CURTAIN
        {
            if locks {
                tracing::info!("the curtain ends: the session locks");
                self.away.under_lock = Some(Ended {
                    pill_output: Some(output.clone()),
                });
                self.curtain_down(false);
                self.lock_now();
            } else {
                let after = self.config.lock_after(self.away.battery);
                tracing::info!(lock_after = ?after, "the curtain ends: the screens go off");
                self.curtain_down(false);
                self.away.phase = Phase::Dark {
                    lock_at: after.map(|d| now + d),
                };
                self.away.screens_off = true;
            }
            self.redraw = true;
        }
        if let Phase::Dark { lock_at: Some(at) } = self.away.phase
            && now >= at
        {
            tracing::info!("the screens have been off for the lock's delay: the session locks");
            self.away.phase = Phase::Here;
            self.lock_at_once();
        }
        if !self.away.screens_off
            && let Some(since) = self.lock_since()
            && now >= since.max(self.idle_input_at()) + SCREEN_OFF_LOCKED
        {
            tracing::info!("locked without input: the screens go off");
            self.away.screens_off = true;
            self.redraw = true;
        }
        if self.away.sleep.until.is_some_and(|until| now >= until) {
            self.let_sleep(true);
        }
        self.away_next()
    }

    /// What is due now, and the timer for what comes next: the held clock
    /// stepped (`Headless::step`).
    pub fn away_check(&mut self) {
        self.away_due();
        self.away_arm();
    }

    /// The timer for the nearest deadline: kept when it fires first (it
    /// sets itself again then), replaced when the deadline is earlier,
    /// removed when there is none.
    fn away_arm(&mut self) {
        let next = self.away_next();
        match (self.away.timer.as_ref().map(|(at, _)| *at), next) {
            (Some(at), Some(next)) if at <= next => return,
            (None, None) => return,
            _ => {}
        }
        if let Some((_, token)) = self.away.timer.take() {
            self.loop_handle.remove(token);
        }
        let Some(at) = next else {
            return;
        };
        let timer = Timer::from_duration(at.saturating_sub(self.now()));
        let inserted = self
            .loop_handle
            .insert_source(timer, |_, _, app| app.state.away_timer_fired());
        match inserted {
            Ok(token) => {
                self.away.timer = Some((at, token));
                self.away.counts.timers += 1;
            }
            Err(err) => tracing::warn!(%err.error, "no timer for the curtain and the screens"),
        }
    }

    fn away_timer_fired(&mut self) -> TimeoutAction {
        self.away.counts.wakeups += 1;
        match self.away_due() {
            Some(at) => {
                if let Some((set, _)) = &mut self.away.timer {
                    *set = at;
                }
                TimeoutAction::ToDuration(at.saturating_sub(self.now()))
            }
            None => {
                self.away.timer = None;
                TimeoutAction::Drop
            }
        }
    }
}
