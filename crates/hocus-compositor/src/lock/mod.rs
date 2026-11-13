//! The lock screen (7.1, docs/design/lock.md): the session locked, every
//! screen showing the lock alone, and the password that opens it.
//!
//! **The entry point** for what locks (7.2's idle and suspend, 7.4's
//! session menu and `Super+L`) is [`State::lock_now`]; [`State::locked`]
//! and [`State::lock_state`] say where it is, [`State::lock_drawn`]
//! whether every screen shows it (nothing of the session any more).
//!
//! - **Locked**, no client gets a key, a click or the pointer, and none is
//!   drawn: every output's elements are the lock's alone
//!   (`State::elements_for`), for the screen, the captures, the shares
//!   and the recordings alike. The keyboard goes to nobody: the compositor
//!   keeps every key but the allowed shortcuts (the levels' keys, the
//!   virtual terminals) and types the password itself, which therefore
//!   never leaves this process; the shell is told only how many
//!   characters to draw. The pointer goes to the shell's lock surface
//!   under it, whose buttons the shell reports.
//! - **The shell** draws one lock surface a screen
//!   ([`hocus_link::LOCK_NAMESPACE`]), and only those of its own
//!   connection count: the compositor hands it that connection as it
//!   starts it (`WAYLAND_SOCKET`). Without them (the shell crashed, not
//!   started yet) a screen is black, the session still locked; the shell
//!   is started again and told the lock as it connects.
//! - **The password** is checked by PAM on a thread of its own
//!   ([`auth`]), zeroed once checked; PAM's faillock refusing says "too
//!   many attempts", and the field waits [`hocus_link::LOCK_WAIT`].
//! - **ext-session-lock-v1** lets another locker (swaylock) hold the lock
//!   ([`external`]): the same rules, its own surfaces and its own check.
//!   Only its unlock opens it; its crash leaves the session locked, under
//!   this lock.
//! - **Motion**: the lock fades in over the desktop's last frame (a
//!   picture taken as it locks, never a live window), and parts from the
//!   middle as it opens ([`draw`]).

pub mod auth;
pub mod draw;
pub mod external;
mod pam;

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use hocus_link::{LOCK_NAMESPACE, LOCK_WAIT, LockAsk, LockFocus, LockPhase, LockShown, ToShell};
use smithay::{
    backend::renderer::element::{Id, memory::MemoryRenderBuffer},
    desktop::{LayerSurface, layer_map_for_output},
    input::{
        keyboard::{FilterResult, Keycode, Keysym, KeysymHandle, ModifiersState, xkb},
        pointer::{CursorImageStatus, MotionEvent},
    },
    output::Output,
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken,
            channel::{self, Event, Sender},
            timer::{TimeoutAction, Timer},
        },
        wayland_server::{DisplayHandle, Resource, protocol::wl_surface::WlSurface},
    },
    utils::{IsAlive, Logical, Point, SERIAL_COUNTER},
    wayland::{compositor::with_states, session_lock::SessionLockManagerState},
};
use zeroize::Zeroizing;

pub use auth::{Authenticator, Outcome};

use crate::{App, State};

/// The most bytes a password takes: its buffer never grows past this, so
/// that it is never copied by a reallocation and left behind unzeroed.
const PASSWORD_MOST: usize = 512;
/// The frozen picture shows this long at most while the shell's lock has
/// not drawn: then the screen is black.
const FROZEN_MOST: Duration = Duration::from_secs(1);

/// Where the lock is, for what locks and what waits for it (7.2, 7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    /// Locked: nothing of the session is drawn nor gets input.
    Locked,
    /// The right password, or the external locker's unlock: the session
    /// is open again, the lock's halves sliding apart over it.
    Opening,
}

/// The lock, owned by the loop.
#[derive(Debug)]
pub struct Lock {
    phase: Phase,
    /// ext-session-lock-v1's global.
    pub(crate) manager: SessionLockManagerState,
    /// The checks' outcomes, back on the loop.
    results: Option<Sender<(u64, Outcome)>>,
    attempts: u64,
    /// The last line the shell was told.
    told: Option<LockShown>,
    /// The keyboard's compose table, for dead keys in a password; made as
    /// the compositor starts.
    compose: Option<Compose>,
    /// The black under the lock, one id for every screen; the halves of
    /// the lock opening.
    pub(crate) black: Id,
    pub(crate) halves: [Id; 2],
    /// The screens that drew a locked frame since it locked.
    pub(crate) drawn_on: std::cell::RefCell<HashSet<String>>,
    /// The screens that drew the lock itself (its surface, not only
    /// black), and those of them that presented such a frame since: what
    /// the machine waits for before it sleeps (7.2).
    pub(crate) full_on: std::cell::RefCell<HashSet<String>>,
    pub(crate) presented_on: std::cell::RefCell<HashSet<String>>,
    /// Commits of the shell's lock surfaces, and checks run, for tests.
    pub commits: u64,
    pub checks: u64,
}

/// The compose table (not `Debug`).
struct Compose(xkb::compose::Table);

impl std::fmt::Debug for Compose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Compose")
    }
}

#[derive(Debug)]
enum Phase {
    Unlocked,
    Locked(Box<Locked>),
    /// The lock's halves slide apart until `until`.
    Opening {
        until: Duration,
    },
}

#[derive(Debug)]
pub(crate) struct Locked {
    pub(crate) holder: Holder,
    form: Form,
    pub(crate) frozen: Frozen,
    /// When the shell's lock surface first drew on each screen: its fade
    /// starts there.
    pub(crate) shown_at: HashMap<String, Duration>,
    pub(crate) since: Duration,
    /// Every screen drew it.
    drawn: bool,
    /// Up at once, without its fade (before the machine sleeps, 7.2).
    at_once: bool,
}

/// Who holds the lock.
#[derive(Debug)]
pub(crate) enum Holder {
    /// This compositor, its shell drawing, PAM checking.
    Own,
    /// A client through ext-session-lock-v1.
    External(external::Holder),
}

/// The desktop's last frame on each screen, taken as it locked.
#[derive(Debug, Default)]
pub(crate) enum Frozen {
    /// To take before the next frame.
    #[default]
    Wanted,
    Taken(HashMap<String, MemoryRenderBuffer>),
    Gone,
}

/// The lock's form: the password typed, where it is.
struct Form {
    password: Zeroizing<String>,
    step: Step,
    /// Wrong passwords so far: each shakes the field again.
    shake: u32,
    focus: LockFocus,
    caps: bool,
    compose: Option<xkb::compose::State>,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the password, nor its length.
        f.debug_struct("Form")
            .field("step", &self.step)
            .field("shake", &self.shake)
            .field("focus", &self.focus)
            .field("caps", &self.caps)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
enum Step {
    Idle,
    Typing,
    /// Attempt `id` is being checked; the dots typed stay drawn.
    Checking {
        id: u64,
        chars: u32,
    },
    Wrong,
    /// PAM said too many attempts: the field waits until then.
    Waiting {
        until: Duration,
        timer: Option<RegistrationToken>,
    },
}

/// What a key does on the lock's form, once the keyboard decided.
#[derive(Debug)]
pub(crate) enum LockKey {
    Text(Zeroizing<String>),
    Erase,
    Clear,
    Enter,
    Space,
    Tab {
        back: bool,
    },
    /// Kept, doing nothing.
    None,
}

impl Lock {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            phase: Phase::Unlocked,
            manager: SessionLockManagerState::new::<State, _>(dh, |_| true),
            results: None,
            attempts: 0,
            told: None,
            compose: compose_table(),
            black: Id::new(),
            halves: [Id::new(), Id::new()],
            drawn_on: std::cell::RefCell::default(),
            full_on: std::cell::RefCell::default(),
            presented_on: std::cell::RefCell::default(),
            commits: 0,
            checks: 0,
        }
    }

    pub(crate) fn locked_state(&self) -> Option<&Locked> {
        match &self.phase {
            Phase::Locked(locked) => Some(locked),
            _ => None,
        }
    }

    pub(crate) fn locked_mut(&mut self) -> Option<&mut Locked> {
        match &mut self.phase {
            Phase::Locked(locked) => Some(locked),
            _ => None,
        }
    }

    /// Whether the desktop's last frame is to be taken before the next
    /// frame (the backends' `lock::draw::freeze`).
    pub fn wants_freeze(&self) -> bool {
        self.locked_state()
            .is_some_and(|l| matches!(l.frozen, Frozen::Wanted))
    }

    /// What the shell was last told.
    pub fn told(&self) -> Option<&LockShown> {
        self.told.as_ref()
    }

    /// The lock opening: its halves part until then.
    pub(crate) fn opening_until(&self) -> Option<Duration> {
        match self.phase {
            Phase::Opening { until } => Some(until),
            _ => None,
        }
    }
}

/// The compose table of the user's locale (`LC_ALL`, `LC_CTYPE`, `LANG`),
/// read once as the compositor starts.
fn compose_table() -> Option<Compose> {
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "C".into());
    if locale.as_encoded_bytes().contains(&0) {
        return None;
    }
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    xkb::compose::Table::new_from_locale(&context, &locale, xkb::compose::COMPILE_NO_FLAGS)
        .ok()
        .map(Compose)
}

/// Starts the lock's results' channel on the loop.
pub fn listen(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<()> {
    let (sender, results) = channel::channel::<(u64, Outcome)>();
    handle
        .insert_source(results, |event, _, app| {
            if let Event::Msg((id, outcome)) = event {
                app.state.lock_checked(id, outcome);
            }
        })
        .map_err(|err| anyhow::anyhow!("could not hear the lock's checks: {}", err.error))?;
    state.lock.results = Some(sender);
    Ok(())
}

impl State {
    // --- The entry point --------------------------------------------------------

    /// Locks the session now (7.2's idle and suspend, 7.4's menu and
    /// `Super+L`): from this call on, no client gets input, and none is
    /// drawn from the next frame on. Locked already, nothing changes.
    pub fn lock_now(&mut self) {
        self.lock_with(Holder::Own, false);
    }

    /// Locks the session now, its lock up at once on every screen, without
    /// its fade (before the machine sleeps, 7.2).
    pub fn lock_at_once(&mut self) {
        self.lock_with(Holder::Own, true);
    }

    /// When the session locked, while it is.
    pub(crate) fn lock_since(&self) -> Option<Duration> {
        self.lock.locked_state().map(|l| l.since)
    }

    /// Whether every screen that is on presented a frame of the lock
    /// itself since it locked (its surface, or black when there is none
    /// to wait for): what the machine waits for before it sleeps (7.2).
    pub fn lock_presented(&self) -> bool {
        if !self.lock_drawn() {
            return false;
        }
        let presented = self.lock.presented_on.borrow();
        self.space
            .outputs()
            .filter(|o| self.output_powered(o))
            .all(|o| presented.contains(&o.name()))
    }

    /// Whether the session is locked (the lock opening is not).
    pub fn locked(&self) -> bool {
        self.lock.locked_state().is_some()
    }

    /// Where the lock is.
    pub fn lock_state(&self) -> LockState {
        match self.lock.phase {
            Phase::Unlocked => LockState::Unlocked,
            Phase::Locked(_) => LockState::Locked,
            Phase::Opening { .. } => LockState::Opening,
        }
    }

    /// Whether every screen has drawn the lock since it locked, its fade
    /// over: nothing of the session shows (what to wait for before the
    /// machine suspends, 7.2).
    pub fn lock_drawn(&self) -> bool {
        self.lock
            .locked_state()
            .is_some_and(|l| l.drawn && !matches!(l.frozen, Frozen::Taken(_) | Frozen::Wanted))
    }

    // --- Locking and unlocking --------------------------------------------------

    pub(crate) fn lock_with(&mut self, holder: Holder, at_once: bool) {
        if self.locked() {
            tracing::debug!("locked already");
            return;
        }
        tracing::info!(
            by = match holder {
                Holder::Own => "the desktop",
                Holder::External(_) => "an ext-session-lock client",
            },
            "the session locks"
        );
        self.perf.event(hocus_perf::EventKind::Locked, "");
        // What takes input or shows over the desktop goes first, while it
        // can still say so to its clients; the session menu's confirmation
        // too (7.4).
        self.lock_closes_everything();
        self.leave_locking();
        let now = self.now();
        let caps = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().caps_lock);
        let compose = self
            .lock
            .compose
            .as_ref()
            .map(|c| xkb::compose::State::new(&c.0, xkb::compose::STATE_NO_FLAGS));
        let mut password = Zeroizing::new(String::new());
        password.reserve_exact(PASSWORD_MOST);
        self.lock.drawn_on.borrow_mut().clear();
        self.lock.full_on.borrow_mut().clear();
        self.lock.presented_on.borrow_mut().clear();
        // The desktop's last frame, for the shell's lock to fade in over:
        // only when the shell is there to draw it soon. An external locker
        // (whose `locked` waits for screens showing nothing of the
        // session) and a lock without the shell come over black.
        let frozen = if matches!(holder, Holder::Own)
            && !at_once
            && self.shell_linked()
            && self.space.outputs().next().is_some()
        {
            Frozen::Wanted
        } else {
            Frozen::Gone
        };
        self.lock.phase = Phase::Locked(Box::new(Locked {
            holder,
            form: Form {
                password,
                step: Step::Idle,
                shake: 0,
                focus: LockFocus::Field,
                caps,
                compose,
            },
            frozen,
            shown_at: HashMap::new(),
            since: now,
            drawn: false,
            at_once,
        }));
        self.lock_input_cut();
        self.tell_lock();
        self.redraw = true;
        // The screens go off in a while (7.2).
        self.away_locked();
    }

    /// Before locking: grabs end, menus close, the switcher, the capture
    /// and the strokes go.
    fn lock_closes_everything(&mut self) {
        self.end_popup_grab();
        self.bar_keyboard_away();
        if self.switching.open.is_some() {
            self.end_switcher(false);
        }
        self.close_radial(None);
        self.radial_menu.press = None;
        self.drop_overview();
        self.capture_away();
        if self.ink.shows() {
            self.ink_clear();
        }
        if let Some(pointer) = self.seat.get_pointer()
            && pointer.is_grabbed()
        {
            let time = self.input_time();
            pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
        }
        self.pointer.kept_buttons.clear();
        self.pointer.frame_cursor = None;
        self.moving = None;
        self.gestures.swiping = None;
    }

    /// Nobody keeps the keyboard or the pointer: they go to the lock's
    /// surfaces only, the cursor back to the arrow.
    fn lock_input_cut(&mut self) {
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, serial);
        }
        self.pointer.cursor = CursorImageStatus::default_named();
        self.lock_keyboard();
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            self.lock_pointer_motion(at, self.input_time());
        }
    }

    /// The session opens: the lock's halves part, input goes back to where
    /// it was.
    pub(crate) fn open_lock(&mut self) {
        let Phase::Locked(locked) = std::mem::replace(&mut self.lock.phase, Phase::Unlocked) else {
            return;
        };
        tracing::info!("the session unlocks");
        self.perf.event(hocus_perf::EventKind::Unlocked, "");
        let mut locked = *locked;
        if let Step::Waiting {
            timer: Some(timer), ..
        } = std::mem::replace(&mut locked.form.step, Step::Idle)
        {
            self.loop_handle.remove(timer);
        }
        drop(locked);
        self.away_unlocked();
        let motion = self.popover_motion();
        let shell_lock = self
            .space
            .outputs()
            .any(|o| self.shell_lock_layer(o).is_some());
        if motion && shell_lock {
            let until = self.now() + hocus_link::LOCK_PART;
            self.lock.phase = Phase::Opening { until };
            self.tell_lock();
        } else {
            self.lock_opened();
        }
        // Input back where it was.
        self.pointer.cursor = CursorImageStatus::default_named();
        let focused = self.model.focused();
        self.focus(focused);
        self.layer_keyboard();
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            self.pointer_motion(at, self.input_time());
        }
        self.redraw = true;
    }

    /// The lock's motion ended: its surfaces go.
    fn lock_opened(&mut self) {
        self.lock.phase = Phase::Unlocked;
        self.lock.told = None;
        self.tell_shell(ToShell::Unlocked);
        self.redraw = true;
    }

    /// Called as motions are judged (`tick`): whether the lock's motion
    /// runs; `None` when it does not hold the screens, the desktop's
    /// motions judged as usual.
    pub(crate) fn lock_tick(&mut self, now: Duration) -> Option<bool> {
        if let Some(until) = self.lock.opening_until() {
            if now >= until {
                self.lock_opened();
                return None;
            }
            self.redraw = true;
            return None;
        }
        let fade = self.lock_fade();
        let locked = self.lock.locked_mut()?;
        let fading = locked.shown_at.values().any(|at| now < *at + fade);
        // The frozen picture goes once every screen faded in, or after a
        // while without the shell.
        if matches!(locked.frozen, Frozen::Taken(_)) {
            let faded = !locked.shown_at.is_empty() && !fading;
            if faded || now >= locked.since + FROZEN_MOST {
                locked.frozen = Frozen::Gone;
                self.redraw = true;
            } else {
                return Some(true);
            }
        }
        Some(fading)
    }

    /// How long the lock fades in.
    pub(crate) fn lock_fade(&self) -> Duration {
        if self.lock.locked_state().is_some_and(|l| l.at_once) {
            Duration::ZERO
        } else if self.popover_motion() {
            hocus_link::LOCK_FADE
        } else {
            hocus_link::LOCK_FADE_REDUCED
        }
    }

    /// After each turn: the screens that drew the lock, and what waits for
    /// them (an external locker's `locked`).
    pub(crate) fn lock_after_turn(&mut self) {
        self.external_after_turn();
        // A screen that is off (7.2) shows nothing of the session: it
        // waits for none.
        let any = self.space.outputs().next().is_some();
        let names: Vec<String> = self
            .space
            .outputs()
            .filter(|o| self.output_powered(o))
            .map(Output::name)
            .collect();
        let drawn_on = self.lock.drawn_on.borrow().clone();
        let Some(locked) = self.lock.locked_mut() else {
            return;
        };
        if locked.drawn || !any || !names.iter().all(|n| drawn_on.contains(n)) {
            return;
        }
        locked.drawn = true;
        tracing::info!("every screen shows the lock");
        self.external_drawn();
    }

    // --- The shell ---------------------------------------------------------------

    /// The shell's lock surface on `output`: of its own connection only.
    pub(crate) fn shell_lock_layer(&self, output: &Output) -> Option<LayerSurface> {
        let map = layer_map_for_output(output);
        map.layers()
            .find(|l| l.namespace() == LOCK_NAMESPACE && self.is_shell_surface(l.wl_surface()))
            .cloned()
    }

    /// Whether `surface` is the shell's, from the connection the
    /// compositor gave it.
    pub(crate) fn is_shell_surface(&self, surface: &WlSurface) -> bool {
        let Some(shell) = &self.shell.client else {
            return false;
        };
        surface.client().is_some_and(|c| {
            c.id() == *shell
                && c.get_data::<crate::state::ClientState>()
                    .is_some_and(|d| d.shell)
                && surface.alive()
        })
    }

    /// A commit of one of the lock's layer surfaces: the shell's first
    /// buffer on a screen starts its fade there.
    pub(crate) fn lock_layer_committed(&mut self, layer: &LayerSurface, output: &Output) {
        if !self.is_shell_surface(layer.wl_surface()) {
            tracing::warn!("a lock surface from another client than the shell: never shown");
            return;
        }
        self.lock.commits += 1;
        let has_buffer = with_states(layer.wl_surface(), |states| {
            states
                .data_map
                .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>()
                .and_then(|d| d.lock().ok().map(|d| d.buffer().is_some()))
                .unwrap_or(false)
        });
        let now = self.now();
        if let Some(locked) = self.lock.locked_mut()
            && has_buffer
        {
            locked.shown_at.entry(output.name()).or_insert(now);
        }
        // The pointer may be over it now.
        if self.locked()
            && let Some(pointer) = self.seat.get_pointer()
        {
            let at = pointer.current_location();
            self.lock_pointer_motion(at, self.input_time());
        }
    }

    /// The shell connected while locked: it is told the lock again.
    pub(crate) fn lock_shell_connected(&mut self) {
        self.lock.told = None;
        if self.locked() || self.lock.opening_until().is_some() {
            self.tell_lock();
        }
    }

    /// The shell went: its lock surfaces with it, the screens black.
    pub(crate) fn lock_shell_gone(&mut self) {
        if let Some(locked) = self.lock.locked_mut() {
            tracing::warn!("the shell went while locked: the screens stay locked, black");
            locked.shown_at.clear();
            self.lock.full_on.borrow_mut().clear();
            self.lock.presented_on.borrow_mut().clear();
            self.redraw = true;
        }
    }

    /// What the shell asks of the lock.
    pub(crate) fn lock_asked(&mut self, ask: LockAsk) {
        match ask {
            LockAsk::Now => self.lock_now(),
            LockAsk::Submit => self.lock_submit(),
            LockAsk::Focus(focus) => {
                if let Some(form) = self.own_form() {
                    form.focus = focus;
                    self.tell_lock();
                }
            }
        }
    }

    /// What the shell may ask while the session is locked: drawing, the
    /// lock's own asks, the next layout, the authentication dialog's
    /// comings and goings (never drawn nor given a key while locked);
    /// nothing that shows or moves a
    /// window, starts or copies anything.
    pub(crate) fn lock_lets(message: &hocus_link::ToCompositor) -> bool {
        use hocus_link::ToCompositor as T;
        matches!(
            message,
            T::Software
                | T::Wake(_)
                | T::Closing(_)
                | T::NextLayout
                | T::Lock(_)
                | T::Meeting(_)
                | T::Auth(_)
                | T::Leave(hocus_link::LeaveAsk::Cancel)
                | T::A11yLine(_)
                | T::A11yEnd
        )
    }

    /// The lock as the shell draws it, told when it changed.
    pub(crate) fn tell_lock(&mut self) {
        let shown = self.lock_shown();
        let Some(shown) = shown else {
            return;
        };
        if self.lock.told.as_ref() == Some(&shown) {
            return;
        }
        self.lock.told = Some(shown.clone());
        self.tell_shell(ToShell::Lock(shown));
    }

    fn lock_shown(&self) -> Option<LockShown> {
        let names: Vec<String> = self.space.outputs().map(Output::name).collect();
        let output =
            crate::panel::primary(&names, &self.primary_wanted(), self.panel.home.as_deref())
                .unwrap_or_default();
        let agents = self.agents.told.as_ref().map_or(0, |list| {
            list.iter().filter(|a| a.state == "waiting").count() as u32
        });
        let common = |phase, chars, caps, shake, focus| LockShown {
            phase,
            chars,
            caps,
            shake,
            focus,
            agents,
            wallpaper: !self.config.plain_background,
            live: self.lock_live(),
            motion: self.popover_motion(),
            output: output.clone(),
        };
        match &self.lock.phase {
            Phase::Unlocked => None,
            Phase::Opening { .. } => {
                Some(common(LockPhase::Opening, 0, false, 0, LockFocus::Field))
            }
            Phase::Locked(locked) => {
                let form = &locked.form;
                let chars = form.password.chars().count() as u32;
                let (phase, chars) = match &form.step {
                    Step::Idle => (LockPhase::Idle, 0),
                    Step::Typing => (LockPhase::Typing, chars),
                    Step::Checking { chars, .. } => (LockPhase::Checking, *chars),
                    Step::Wrong => (LockPhase::Wrong, chars),
                    Step::Waiting { until, .. } => {
                        let left = until.saturating_sub(self.now());
                        let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
                        (LockPhase::Waiting(seconds as u32), 0)
                    }
                };
                Some(common(phase, chars, form.caps, form.shake, form.focus))
            }
        }
    }

    /// The agents changed: the lock's count may have.
    pub(crate) fn lock_agents_changed(&mut self) {
        if self.locked() {
            self.tell_lock();
        }
    }

    // --- The keyboard -------------------------------------------------------------

    /// Our own lock's form, while it holds the lock.
    fn own_form(&mut self) -> Option<&mut Form> {
        let locked = self.lock.locked_mut()?;
        matches!(locked.holder, Holder::Own).then_some(&mut locked.form)
    }

    /// Whether this compositor's own lock holds the keyboard (an external
    /// locker's surface gets the keys).
    pub(crate) fn lock_keeps_keys(&self) -> bool {
        self.lock
            .locked_state()
            .is_some_and(|l| matches!(l.holder, Holder::Own))
    }

    /// What a key does while locked, inside the keyboard's filter: `None`
    /// when not locked. The allowed shortcuts and the virtual terminals
    /// run; with our own lock every other key is kept (and typed into
    /// the form), with an external one it goes to its surface.
    pub(crate) fn lock_key_filter(
        &mut self,
        code: Keycode,
        pressed: bool,
        mods: &ModifiersState,
        keysym: &KeysymHandle<'_>,
    ) -> Option<FilterResult<crate::input::Kept>> {
        use crate::input::Kept;
        if !self.locked() {
            return None;
        }
        let own = self.lock_keeps_keys();
        if !pressed {
            return Some(if self.keyboard.kept_keys.remove(&code) || own {
                FilterResult::Intercept(Kept::Release)
            } else {
                FilterResult::Forward
            });
        }
        let sym = keysym.modified_sym();
        let raw = sym.raw();
        if (Keysym::XF86_Switch_VT_1.raw()..=Keysym::XF86_Switch_VT_12.raw()).contains(&raw) {
            self.keyboard.kept_keys.insert(code);
            let vt = (raw - Keysym::XF86_Switch_VT_1.raw() + 1) as i32;
            return Some(FilterResult::Intercept(Kept::Vt(vt)));
        }
        let shortcuts = &self.config.shortcuts;
        // The levels' keys and the magnifier's (#424) work locked.
        let level = keysym
            .raw_latin_sym_or_raw_current_sym()
            .and_then(|key| shortcuts.action(mods, key, self.nested))
            .filter(|(action, _)| matches!(action, crate::Action::Level(_)))
            .or_else(|| self.magnifier_chord(mods, keysym));
        if let Some((action, chord)) = level {
            self.keyboard.kept_keys.insert(code);
            return Some(FilterResult::Intercept(Kept::Shortcut(action, chord)));
        }
        if !own {
            return Some(FilterResult::Forward);
        }
        self.keyboard.kept_keys.insert(code);
        Some(FilterResult::Intercept(Kept::Lock(
            self.lock_key_of(sym, mods),
        )))
    }

    /// What `sym` does on the form, through the compose table for dead
    /// keys.
    fn lock_key_of(&mut self, sym: Keysym, mods: &ModifiersState) -> LockKey {
        // A modifier alone, or a chord with Super, Ctrl or Alt (a shortcut,
        // not typing): kept, doing nothing. AltGr's characters are Level3,
        // not Alt.
        if crate::switcher::is_modifier(sym) || mods.logo || mods.ctrl || mods.alt {
            return LockKey::None;
        }
        let Some(form) = self.own_form() else {
            return LockKey::None;
        };
        if let Some(compose) = &mut form.compose
            && compose.feed(sym) == xkb::compose::FeedResult::Accepted
        {
            match compose.status() {
                xkb::compose::Status::Composing => return LockKey::None,
                xkb::compose::Status::Composed => {
                    let text = compose.utf8().map(Zeroizing::new);
                    compose.reset();
                    return text.map_or(LockKey::None, LockKey::Text);
                }
                xkb::compose::Status::Cancelled => {
                    compose.reset();
                    return LockKey::None;
                }
                xkb::compose::Status::Nothing => {}
            }
        }
        match sym {
            Keysym::Return | Keysym::KP_Enter => LockKey::Enter,
            Keysym::Escape => LockKey::Clear,
            Keysym::BackSpace => LockKey::Erase,
            Keysym::Tab => LockKey::Tab { back: mods.shift },
            Keysym::ISO_Left_Tab => LockKey::Tab { back: true },
            Keysym::space => LockKey::Space,
            _ => match sym.key_char() {
                Some(c) if !c.is_control() => {
                    let mut text = Zeroizing::new(String::new());
                    text.push(c);
                    LockKey::Text(text)
                }
                _ => LockKey::None,
            },
        }
    }

    /// A key the lock kept, after the keyboard's filter.
    pub(crate) fn lock_key(&mut self, key: LockKey) {
        let layouts = self.keyboard.layout_codes.len() > 1;
        let Some(form) = self.own_form() else {
            return;
        };
        let takes = matches!(form.step, Step::Idle | Step::Typing | Step::Wrong);
        match key {
            LockKey::None => {}
            LockKey::Text(text) if takes => {
                if form.password.len() + text.len() <= PASSWORD_MOST {
                    form.password.push_str(&text);
                    form.step = Step::Typing;
                    form.focus = LockFocus::Field;
                }
            }
            LockKey::Space if takes && form.focus == LockFocus::Field => {
                if form.password.len() < PASSWORD_MOST {
                    form.password.push(' ');
                    form.step = Step::Typing;
                }
            }
            LockKey::Space | LockKey::Enter if form.focus == LockFocus::Layout => {
                self.next_layout();
            }
            LockKey::Space | LockKey::Enter => self.lock_submit(),
            LockKey::Erase if takes => {
                form.password.pop();
                form.step = if form.password.is_empty() {
                    Step::Idle
                } else {
                    Step::Typing
                };
            }
            LockKey::Clear if takes => {
                clear(&mut form.password);
                form.step = Step::Idle;
                form.focus = LockFocus::Field;
            }
            LockKey::Tab { back } => form.focus = form.focus.next(back, layouts),
            LockKey::Text(_) | LockKey::Erase | LockKey::Clear => {}
        }
        self.tell_lock();
    }

    /// After each key: Caps Lock as the keyboard says it now.
    pub(crate) fn lock_modifiers(&mut self, mods: &ModifiersState) {
        if let Some(form) = self.own_form()
            && form.caps != mods.caps_lock
        {
            form.caps = mods.caps_lock;
            self.tell_lock();
        }
    }

    /// The keyboard to the lock's surface: nobody with our own lock (the
    /// compositor keeps the keys), the external locker's surface under the
    /// pointer with theirs. Whether the lock decided.
    pub(crate) fn lock_keyboard(&mut self) -> bool {
        if !self.locked() {
            return false;
        }
        let focus = self
            .external_keyboard_surface()
            .map(crate::focus::KeyboardFocus::Wayland);
        if let Some(keyboard) = self.seat.get_keyboard()
            && keyboard.current_focus() != focus
        {
            keyboard.set_focus(self, focus, SERIAL_COUNTER.next_serial());
        }
        true
    }

    // --- The pointer -----------------------------------------------------------------

    /// The lock's surface under `at`, and where its origin is.
    pub(crate) fn lock_surface_under(
        &self,
        at: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.output_under(at).next()?;
        let origin = self.space.output_geometry(output)?.loc;
        if let Some(surface) = self.external_surface_on(output) {
            return smithay::desktop::utils::under_from_surface_tree(
                &surface,
                at - origin.to_f64(),
                (0, 0),
                smithay::desktop::WindowSurfaceType::ALL,
            )
            .map(|(s, p)| (s, (p + origin).to_f64()));
        }
        let layer = self.shell_lock_layer(output)?;
        let map = layer_map_for_output(output);
        let geometry = map.layer_geometry(&layer)?;
        let loc = (origin + geometry.loc).to_f64();
        layer
            .surface_under(at - loc, smithay::desktop::WindowSurfaceType::ALL)
            .map(|(s, p)| (s, (p + geometry.loc + origin).to_f64()))
    }

    /// The pointer moved while locked: to the lock's surface under it.
    pub(crate) fn lock_pointer_motion(&mut self, location: Point<f64, Logical>, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let under = self.lock_surface_under(location);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
        if !self.nested {
            self.redraw = true;
        }
    }

    // --- The check -------------------------------------------------------------------

    /// `Enter` or the arrow: what was typed is checked, off the loop.
    fn lock_submit(&mut self) {
        let auth = self.config.lock_auth.clone();
        let Some(results) = self.lock.results.clone() else {
            tracing::warn!("the lock cannot check: no channel");
            return;
        };
        self.lock.attempts += 1;
        let id = self.lock.attempts;
        let Some(form) = self.own_form() else {
            return;
        };
        if !matches!(form.step, Step::Typing) || form.password.is_empty() {
            return;
        }
        let chars = form.password.chars().count() as u32;
        let mut password = Zeroizing::new(String::new());
        password.reserve_exact(PASSWORD_MOST);
        std::mem::swap(&mut password, &mut form.password);
        if let Some(compose) = &mut form.compose {
            compose.reset();
        }
        form.step = Step::Checking { id, chars };
        self.lock.checks += 1;
        tracing::info!(attempt = id, "the lock checks a password");
        crate::programs::on_a_thread("lock's check".to_owned(), move || {
            let outcome = match auth::user() {
                Some(user) => auth.check(&user, &password),
                None => Outcome::Failed("no account for this user".to_owned()),
            };
            drop(password);
            let _ = results.send((id, outcome));
        });
        self.tell_lock();
    }

    /// The check of attempt `id` says `outcome`.
    fn lock_checked(&mut self, id: u64, outcome: Outcome) {
        let now = self.now();
        let Some(form) = self.own_form() else {
            return;
        };
        if !matches!(form.step, Step::Checking { id: checking, .. } if checking == id) {
            tracing::debug!(attempt = id, "a check of another time");
            return;
        }
        match outcome {
            Outcome::Right => {
                tracing::info!(attempt = id, "the password is right");
                self.open_lock();
                return;
            }
            Outcome::Wrong => {
                tracing::info!(attempt = id, "a wrong password");
                form.step = Step::Wrong;
                form.shake += 1;
            }
            Outcome::Failed(why) => {
                tracing::warn!(attempt = id, why, "the password could not be checked");
                form.step = Step::Wrong;
                form.shake += 1;
            }
            Outcome::TooMany => {
                tracing::info!(attempt = id, "PAM says too many attempts: the field waits");
                form.step = Step::Waiting {
                    until: now + LOCK_WAIT,
                    timer: None,
                };
                self.lock_wait_tick();
            }
        }
        self.tell_lock();
    }

    /// While the field waits: the seconds left, told as each passes, and
    /// the field back at the end.
    pub(crate) fn lock_wait_tick(&mut self) {
        let now = self.now();
        let (until, old) = match self.own_form() {
            Some(Form {
                step: Step::Waiting { until, timer },
                ..
            }) => (*until, timer.take()),
            _ => return,
        };
        if let Some(old) = old {
            self.loop_handle.remove(old);
        }
        if now >= until {
            tracing::info!("the field takes a password again");
            if let Some(form) = self.own_form() {
                form.step = Step::Idle;
            }
            self.tell_lock();
            return;
        }
        // At the next whole second left.
        let next = match (until - now).subsec_nanos() {
            0 => Duration::from_secs(1),
            nanos => Duration::from_nanos(u64::from(nanos)),
        };
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(next), |_, _, app| {
                if let Some(Form {
                    step: Step::Waiting { timer, .. },
                    ..
                }) = app.state.own_form()
                {
                    // This timer goes as it returns.
                    *timer = None;
                }
                app.state.lock_wait_tick();
                TimeoutAction::Drop
            });
        match inserted {
            Ok(token) => {
                if let Some(Form {
                    step: Step::Waiting { timer, .. },
                    ..
                }) = self.own_form()
                {
                    *timer = Some(token);
                }
            }
            Err(err) => tracing::warn!(%err.error, "could not count the lock's wait down"),
        }
        self.tell_lock();
    }
}

/// Empties a password, its bytes zeroed.
fn clear(password: &mut Zeroizing<String>) {
    use zeroize::Zeroize;
    password.zeroize();
}

impl State {
    /// For tests: the password's length, never its text.
    pub(crate) fn lock_typed(&self) -> Option<usize> {
        let locked = self.lock.locked_state()?;
        Some(locked.form.password.chars().count())
    }

    /// The screens' names that drew the lock since it locked; `full`: the
    /// lock itself, not only black.
    pub(crate) fn lock_drew_on(&self, output: &Output, full: bool) {
        if self.locked() {
            self.lock.drawn_on.borrow_mut().insert(output.name());
            if full {
                self.lock.full_on.borrow_mut().insert(output.name());
            }
        }
    }

    /// A frame of `output` was presented: if it showed the lock itself, the
    /// screen shows it now.
    pub(crate) fn lock_presented_on(&self, output: &Output) {
        if self.locked() && self.lock.full_on.borrow().contains(&output.name()) {
            self.lock.presented_on.borrow_mut().insert(output.name());
        }
    }

    /// The machine woke up locked (7.2): the field holds the keyboard.
    pub(crate) fn lock_woke(&mut self) {
        if let Some(form) = self.own_form() {
            form.focus = LockFocus::Field;
            self.tell_lock();
        }
    }

    /// Whether `surface` may be told frames while locked: the lock's
    /// surfaces and the cursor's.
    pub(crate) fn lock_frames_for(&self, surface: &WlSurface) -> bool {
        if !self.locked() {
            return true;
        }
        if self.is_shell_surface(surface) {
            return true;
        }
        self.external_owns(surface)
    }
}

/// The black under the lock.
pub(crate) const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_buffer_never_grows() {
        let mut password = Zeroizing::new(String::new());
        password.reserve_exact(PASSWORD_MOST);
        let capacity = password.capacity();
        for _ in 0..PASSWORD_MOST {
            password.push('a');
        }
        assert_eq!(password.capacity(), capacity);
        clear(&mut password);
        assert!(password.is_empty());
    }
}
