//! How the session ends (7.4, the session menu, docs/design/bar.md): the
//! compositor's side. The shell draws the menu in the user's popover and
//! says what is chosen; here the confirmation counts down, applications
//! that hold it back are listed, windows are asked to close and waited
//! for, and logind is called ([`logind`], off the loop).
//!
//! - **Lock** is [`State::lock_now`] (the shell's `lock now`, `Super+L`).
//! - **Suspend** locks, waits for every screen to show the lock (2 s at
//!   most), then asks logind to suspend.
//! - **Log out, restart, shut down** ask for confirmation: 10 s counted
//!   on the motions' clock (`State::now`, which tests hold), cancellable.
//!   Restart and shut down first ask polkit for the password when logind
//!   says `challenge`, and are held back while logind lists `block`
//!   inhibitors of `shutdown`; the countdown starts again from 10 s when
//!   the last goes. At 0 (or "… maintenant") every window is asked to
//!   close and given 5 s; those still open are listed as holding it back,
//!   with "Revenir à …" and "Fermer quand même"; once none is left, or on
//!   "… quand même", it happens: the compositor ends (log out), or logind
//!   restarts or powers the machine off and then the compositor ends.
//! - `Super+Shift+E` opens the log-out confirmation; without a shell to
//!   show it, it logs out at once as before.
//! - The confirmation goes when the session locks, or when the shell
//!   goes: nobody could cancel it any more.

pub mod logind;

use std::time::Duration;

use hocus_link::{
    Can, Holder, HolderKind, LEAVE_CLOSE_WAIT, LEAVE_COUNTDOWN, LeaveAction, LeaveAsk, LeaveCan,
    LeaveShown, LeaveStep, ToShell,
};
use smithay::reexports::{
    calloop::{
        RegistrationToken,
        channel::{self, Event},
        timer::{TimeoutAction, Timer},
    },
    wayland_server::{Resource, backend::DisconnectReason},
};

use smithay::wayland::seat::WaylandFocus;

use crate::{Ending, State, model::WindowId};

use logind::{Act, Ask, Logind, Said};

/// Suspending waits this long at most for every screen to show the lock.
const LOCK_BEFORE_SUSPEND: Duration = Duration::from_secs(2);

/// Where the session's end is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    /// polkit asks for the password before the countdown (`challenge`).
    Authorizing { action: LeaveAction, output: String },
    /// Counting down to `until` (the motions' clock).
    Counting {
        action: LeaveAction,
        output: String,
        until: Duration,
    },
    /// logind's inhibitors hold it back: nothing counts.
    Inhibited { action: LeaveAction, output: String },
    /// Every window was asked to close; waited for until `until`.
    Closing {
        action: LeaveAction,
        output: String,
        until: Duration,
        windows: Vec<WindowId>,
    },
    /// Windows still open after 5 s, listed.
    Holding {
        action: LeaveAction,
        output: String,
        windows: Vec<WindowId>,
    },
    /// logind called, its answer awaited.
    Acting { action: LeaveAction, output: String },
    /// Locked since `since`, the machine to sleep once the lock shows.
    Suspending { since: Duration },
}

/// The session's end, as the loop keeps it.
#[derive(Debug, Default)]
pub struct Leave {
    phase: Phase,
    /// What logind allows, last read.
    can: LeaveCan,
    told_can: Option<LeaveCan>,
    /// What the shell was told last of a confirmation.
    told: Option<ToShell>,
    /// logind's inhibitors holding `shutdown`, while watched.
    inhibitors: Vec<logind::Inhibitor>,
    logind: Option<Logind>,
    timer: Option<RegistrationToken>,
}

impl Phase {
    fn action(&self) -> Option<LeaveAction> {
        match self {
            Self::Authorizing { action, .. }
            | Self::Counting { action, .. }
            | Self::Inhibited { action, .. }
            | Self::Closing { action, .. }
            | Self::Holding { action, .. }
            | Self::Acting { action, .. } => Some(*action),
            Self::Idle | Self::Suspending { .. } => None,
        }
    }

    fn output(&self) -> Option<&str> {
        match self {
            Self::Authorizing { output, .. }
            | Self::Counting { output, .. }
            | Self::Inhibited { output, .. }
            | Self::Closing { output, .. }
            | Self::Holding { output, .. }
            | Self::Acting { output, .. } => Some(output),
            Self::Idle | Self::Suspending { .. } => None,
        }
    }
}

impl State {
    /// logind on `bus` (an address, or `system`; none: no power actions),
    /// what it allows read at once.
    pub(crate) fn leave_start(&mut self, bus: Option<String>) {
        let Some(bus) = bus else {
            tracing::info!("no system bus given: no power actions in the session menu");
            return;
        };
        let (sender, receiver) = channel::channel::<Said>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.leave_said(said);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "could not listen to logind");
            return;
        }
        self.leave.logind = Logind::start(bus, sender);
        self.leave_logind(Ask::Can);
    }

    pub(crate) fn leave_logind(&self, ask: Ask) {
        if let Some(logind) = &self.leave.logind {
            logind.ask(ask);
        }
    }

    /// What the shell asks of the session menu.
    pub(crate) fn leave_asked(&mut self, ask: LeaveAsk) {
        tracing::debug!(?ask, phase = ?self.leave.phase, "the session menu");
        match ask {
            LeaveAsk::Open => self.leave_logind(Ask::Can),
            LeaveAsk::Ask { action, output } => self.leave_ask(action, output),
            LeaveAsk::Suspend => self.leave_suspend(),
            LeaveAsk::Now => self.leave_now(),
            LeaveAsk::Cancel => self.leave_cancel("cancelled"),
            LeaveAsk::Force(id) => self.leave_force(WindowId(id)),
        }
    }

    /// `Super+Shift+E`: the log-out confirmation on the screen of the
    /// focused window (else the pointer's); without a shell to show it,
    /// the session ends at once.
    pub(crate) fn leave_shortcut(&mut self) {
        if !self.shell_linked() {
            tracing::info!("no shell to confirm: the session ends");
            self.end(Ending::LoggedOut);
            return;
        }
        if self.leave.phase != Phase::Idle {
            return;
        }
        let Some(output) = self.bubbles_screen() else {
            self.end(Ending::LoggedOut);
            return;
        };
        self.leave_ask(LeaveAction::LogOut, output.name());
    }

    /// A row chosen: its confirmation, the password first when logind
    /// wants it.
    fn leave_ask(&mut self, action: LeaveAction, output: String) {
        if !matches!(self.leave.phase, Phase::Idle) {
            tracing::debug!(?action, "a confirmation shows already");
            return;
        }
        let can = self.leave.can.of(action);
        if !can.allowed() {
            tracing::info!(?action, "logind does not allow it");
            return;
        }
        tracing::info!(?action, "the session menu asks to confirm");
        if action != LeaveAction::LogOut {
            self.leave.inhibitors.clear();
            self.leave_logind(Ask::Watch(true));
        }
        if can == Can::Challenge {
            self.leave.phase = Phase::Authorizing { action, output };
            self.leave_logind(Ask::Authorize(action));
            self.leave_tell();
            return;
        }
        self.leave_count(action, output);
    }

    /// The countdown, from 10 s.
    fn leave_count(&mut self, action: LeaveAction, output: String) {
        let until = self.now() + LEAVE_COUNTDOWN;
        self.leave.phase = if self.leave_inhibited(action) {
            Phase::Inhibited { action, output }
        } else {
            Phase::Counting {
                action,
                output,
                until,
            }
        };
        self.leave_tick();
    }

    /// Whether logind's inhibitors hold `action` back.
    fn leave_inhibited(&self, action: LeaveAction) -> bool {
        action != LeaveAction::LogOut && !self.leave.inhibitors.is_empty()
    }

    /// "… maintenant" or "… quand même": past the countdown and the
    /// inhibitors the windows are asked to close; past the windows it
    /// happens.
    fn leave_now(&mut self) {
        match std::mem::take(&mut self.leave.phase) {
            Phase::Counting { action, output, .. } | Phase::Inhibited { action, output } => {
                tracing::info!(?action, "at once");
                self.leave_close_windows(action, output);
            }
            Phase::Closing { action, output, .. } | Phase::Holding { action, output, .. } => {
                tracing::info!(?action, "without waiting for the windows");
                self.leave_act(action, output);
            }
            other => self.leave.phase = other,
        }
    }

    /// Nothing ends: the menu's rows again.
    pub(crate) fn leave_cancel(&mut self, why: &str) {
        let phase = std::mem::take(&mut self.leave.phase);
        match phase {
            Phase::Idle | Phase::Suspending { .. } => {
                self.leave.phase = phase;
                return;
            }
            // Too late: logind has it.
            Phase::Acting { .. } => {
                self.leave.phase = phase;
                return;
            }
            _ => {}
        }
        tracing::info!(why, action = ?phase.action(), "the session's end is cancelled");
        self.leave_logind(Ask::Watch(false));
        self.leave.inhibitors.clear();
        self.leave_disarm();
        self.leave_tell();
    }

    /// "Mettre en veille": the session locks first, then the machine
    /// sleeps; with `[power] lock-before-sleep = false`, it sleeps at
    /// once.
    fn leave_suspend(&mut self) {
        if !self.leave.can.suspend.allowed() || self.leave.phase != Phase::Idle {
            return;
        }
        if !self.config.power.lock_before_sleep {
            tracing::info!("suspend, unlocked (lock-before-sleep off)");
            self.leave_logind(Ask::Act(Act::Suspend));
            return;
        }
        tracing::info!("suspend: the session locks first");
        self.lock_now();
        self.leave.phase = Phase::Suspending { since: self.now() };
        self.leave_tick();
    }

    /// Every window asked to close, given 5 s.
    fn leave_close_windows(&mut self, action: LeaveAction, output: String) {
        self.leave_logind(Ask::Watch(false));
        let windows: Vec<WindowId> = self.windows.keys().copied().collect();
        tracing::info!(count = windows.len(), "every window is asked to close");
        for id in &windows {
            self.leave_close_one(*id);
        }
        self.leave.phase = Phase::Closing {
            action,
            output,
            until: self.now() + LEAVE_CLOSE_WAIT,
            windows,
        };
        self.leave_after_turn();
        self.leave_tick();
    }

    fn leave_close_one(&self, id: WindowId) {
        let Some(managed) = self.windows.get(&id) else {
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

    /// "Fermer quand même": the window's client is cut off (an X11
    /// window's process ended).
    fn leave_force(&mut self, id: WindowId) {
        let Phase::Holding { windows, .. } = &self.leave.phase else {
            return;
        };
        if !windows.contains(&id) {
            return;
        }
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        tracing::info!(?id, "a window that does not close is closed all the same");
        if let Some(x11) = managed.window.x11_surface() {
            match x11
                .pid()
                .and_then(|pid| rustix::process::Pid::from_raw(pid as i32))
            {
                Some(pid) => {
                    if let Err(err) =
                        rustix::process::kill_process(pid, rustix::process::Signal::KILL)
                    {
                        tracing::warn!(%err, "could not end an X11 window's process");
                    }
                }
                None => tracing::warn!("an X11 window without a process"),
            }
        } else if let Some(surface) = managed.window.wl_surface()
            && let Ok(client) = self.display_handle.get_client(surface.id())
        {
            self.display_handle
                .backend_handle()
                .kill_client(client.id(), DisconnectReason::ConnectionClosed);
        }
    }

    /// The power button, `[power] power-button = "power-off"` (6.10): the
    /// shutdown as the session menu does it after its countdown, every
    /// window asked to close first; the password first when polkit wants
    /// it (then the countdown, as the menu's).
    pub(crate) fn leave_power_off(&mut self) {
        if self.leave.phase != Phase::Idle {
            return;
        }
        let output = self.bubbles_screen().map(|o| o.name()).unwrap_or_default();
        match self.leave.can.shut_down {
            Can::Challenge => self.leave_ask(LeaveAction::ShutDown, output),
            can if can.allowed() => {
                tracing::info!("the power button: the machine powers off");
                self.leave_close_windows(LeaveAction::ShutDown, output);
            }
            _ => tracing::info!("the power button: logind does not allow powering off"),
        }
    }

    /// It happens: the session ends, or logind is asked first.
    fn leave_act(&mut self, action: LeaveAction, output: String) {
        self.leave_disarm();
        tracing::info!(?action, "the session ends");
        match action {
            LeaveAction::LogOut => {
                self.leave.phase = Phase::Acting { action, output };
                self.leave_tell();
                self.end(Ending::LoggedOut);
            }
            LeaveAction::Restart | LeaveAction::ShutDown => {
                self.leave.phase = Phase::Acting { action, output };
                self.leave_tell();
                self.leave_logind(Ask::Act(if action == LeaveAction::Restart {
                    Act::Restart
                } else {
                    Act::ShutDown
                }));
            }
        }
    }

    /// What logind said.
    fn leave_said(&mut self, said: Said) {
        match said {
            Said::Can(can) => {
                self.leave.can = can;
                self.leave_tell_can();
            }
            Said::Inhibitors(list) => {
                if list != self.leave.inhibitors {
                    tracing::info!(count = list.len(), "logind's inhibitors hold it back");
                }
                self.leave.inhibitors = list;
                match std::mem::take(&mut self.leave.phase) {
                    Phase::Counting { action, output, .. } if self.leave_inhibited(action) => {
                        self.leave.phase = Phase::Inhibited { action, output };
                        self.leave_disarm();
                    }
                    // The last let go: from 10 s again.
                    Phase::Inhibited { action, output } if !self.leave_inhibited(action) => {
                        self.leave_count(action, output);
                        return;
                    }
                    other => self.leave.phase = other,
                }
                self.leave_tell();
            }
            Said::Authorized(action, granted) => {
                let Phase::Authorizing {
                    action: asked,
                    output,
                } = &self.leave.phase
                else {
                    return;
                };
                if *asked != action {
                    return;
                }
                if granted {
                    let output = output.clone();
                    self.leave_count(action, output);
                } else {
                    self.leave_cancel("not authorized");
                }
            }
            Said::Acted(act, Ok(())) => {
                tracing::info!(?act, "logind took it");
                match act {
                    Act::Suspend => {}
                    Act::Restart => self.end(Ending::Restarted),
                    Act::ShutDown => self.end(Ending::PoweredOff),
                }
            }
            Said::SleepHeld(held) => {
                let who: Vec<&str> = held.iter().map(|i| i.who.as_str()).collect();
                tracing::info!(
                    ?who,
                    "logind's idle inhibitors keep the machine awake: later"
                );
                self.idle_suspend_put_off();
            }
            Said::Acted(act, Err(err)) => {
                tracing::warn!(?act, err, "logind refused");
                if matches!(self.leave.phase, Phase::Acting { .. }) {
                    self.leave.phase = Phase::Idle;
                    self.leave_tell();
                }
            }
        }
    }

    /// What falls due: the countdown's seconds, the windows' 5 s, the
    /// lock before suspending; the timer set for the next.
    pub(crate) fn leave_tick(&mut self) {
        let now = self.now();
        self.leave_disarm();
        let next = match &self.leave.phase {
            Phase::Counting {
                action,
                output,
                until,
            } => {
                if now >= *until {
                    let (action, output) = (*action, output.clone());
                    self.leave_close_windows(action, output);
                    return;
                }
                // At the next whole second left.
                match (*until - now).subsec_nanos() {
                    0 => Some(Duration::from_secs(1)),
                    nanos => Some(Duration::from_nanos(u64::from(nanos))),
                }
            }
            Phase::Closing { until, .. } => {
                if now >= *until {
                    self.leave_hold();
                    return;
                }
                Some(*until - now)
            }
            Phase::Suspending { since } => {
                if self.lock_drawn() || now >= *since + LOCK_BEFORE_SUSPEND {
                    tracing::info!(drawn = self.lock_drawn(), "the machine sleeps");
                    self.leave.phase = Phase::Idle;
                    self.leave_logind(Ask::Act(Act::Suspend));
                    return;
                }
                Some(Duration::from_millis(50).min(*since + LOCK_BEFORE_SUSPEND - now))
            }
            _ => None,
        };
        self.leave_tell();
        let Some(next) = next else {
            return;
        };
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(next), |_, _, app| {
                // This timer goes as it returns.
                app.state.leave.timer = None;
                app.state.leave_tick();
                TimeoutAction::Drop
            });
        match inserted {
            Ok(token) => self.leave.timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "could not count the session's end down"),
        }
    }

    fn leave_disarm(&mut self) {
        if let Some(timer) = self.leave.timer.take() {
            self.loop_handle.remove(timer);
        }
    }

    /// The windows' 5 s are over: those still open hold it back.
    fn leave_hold(&mut self) {
        let Phase::Closing {
            action,
            output,
            windows,
            ..
        } = std::mem::take(&mut self.leave.phase)
        else {
            return;
        };
        let windows: Vec<WindowId> = windows
            .into_iter()
            .filter(|id| self.windows.contains_key(id))
            .collect();
        if windows.is_empty() {
            self.leave_act(action, output);
            return;
        }
        tracing::info!(
            count = windows.len(),
            "windows did not close: they hold it back"
        );
        self.leave.phase = Phase::Holding {
            action,
            output,
            windows,
        };
        self.leave_tell();
    }

    /// After each turn: windows that closed leave the list; once none is
    /// left it happens. The lock drawn, the machine sleeps.
    pub(crate) fn leave_after_turn(&mut self) {
        let (gone, left) = match &mut self.leave.phase {
            Phase::Idle => return,
            Phase::Closing { windows, .. } | Phase::Holding { windows, .. } => {
                let open = &self.windows;
                let before = windows.len();
                windows.retain(|id| open.contains_key(id));
                (windows.len() != before, windows.len())
            }
            Phase::Suspending { .. } => {
                if self.lock_drawn() {
                    self.leave_tick();
                }
                return;
            }
            _ => return,
        };
        if left == 0 {
            if let Phase::Closing { action, output, .. } | Phase::Holding { action, output, .. } =
                std::mem::take(&mut self.leave.phase)
            {
                self.leave_act(action, output);
            }
        } else if gone {
            self.leave_tell();
        }
    }

    /// The shell told what a confirmation shows, when it changed.
    fn leave_tell(&mut self) {
        let shown = self.leave_shown();
        if self.leave.told.as_ref() == Some(&shown) {
            return;
        }
        // Nothing under way, and nothing was: nothing to say.
        if shown == ToShell::LeaveEnd && self.leave.told.is_none() {
            return;
        }
        self.leave.told = Some(shown.clone());
        self.tell_shell(shown);
    }

    fn leave_shown(&self) -> ToShell {
        let phase = &self.leave.phase;
        let (Some(action), Some(output)) = (phase.action(), phase.output()) else {
            return ToShell::LeaveEnd;
        };
        let now = self.now();
        let step = match phase {
            Phase::Authorizing { .. } => LeaveStep::Authorizing,
            Phase::Counting { until, .. } => LeaveStep::Counting {
                left_ms: u32::try_from(until.saturating_sub(now).as_millis()).unwrap_or(u32::MAX),
            },
            Phase::Inhibited { .. } => LeaveStep::Held(
                self.leave
                    .inhibitors
                    .iter()
                    .map(|i| Holder {
                        kind: HolderKind::Inhibitor { pid: i.pid },
                        app: i.who.clone(),
                        text: i.why.clone(),
                    })
                    .collect(),
            ),
            Phase::Holding { windows, .. } => LeaveStep::Held(
                windows
                    .iter()
                    .filter_map(|id| {
                        let managed = self.windows.get(id)?;
                        let app = crate::windows::app_id(&managed.window).unwrap_or_default();
                        // Its title, if it has one (not its id standing in).
                        let title = crate::windows::title_of(self, *id);
                        Some(Holder {
                            kind: HolderKind::Window { id: id.0 },
                            text: if title == app { String::new() } else { title },
                            app,
                        })
                    })
                    .collect(),
            ),
            Phase::Closing { .. } => LeaveStep::Closing,
            Phase::Acting { .. } => LeaveStep::Acting,
            Phase::Idle | Phase::Suspending { .. } => return ToShell::LeaveEnd,
        };
        ToShell::Leave(LeaveShown {
            output: output.to_owned(),
            action,
            step,
        })
    }

    fn leave_tell_can(&mut self) {
        if self.leave.told_can == Some(self.leave.can) {
            return;
        }
        self.leave.told_can = Some(self.leave.can);
        self.tell_shell(ToShell::LeaveCan(self.leave.can));
    }

    /// The shell connected: what logind allows, and the confirmation if
    /// one is under way (none: a shell gone took it with it).
    pub(crate) fn leave_shell_connected(&mut self) {
        self.leave.told_can = None;
        self.leave_tell_can();
        self.leave.told = None;
        self.leave_tell();
    }

    /// The shell went: nobody can cancel a confirmation any more.
    pub(crate) fn leave_shell_gone(&mut self) {
        self.leave_cancel("the shell went");
        self.leave.told = None;
    }

    /// The session locks: a confirmation goes (not the suspend's own
    /// lock).
    pub(crate) fn leave_locking(&mut self) {
        self.leave_cancel("the session locks");
    }

    /// Where the session's end is, for tests: the action of a
    /// confirmation under way and what it shows.
    pub fn leave_shown_now(&self) -> Option<LeaveShown> {
        match self.leave_shown() {
            ToShell::Leave(shown) => Some(shown),
            _ => None,
        }
    }
}
