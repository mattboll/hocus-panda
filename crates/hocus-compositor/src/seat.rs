//! MCP agents' own seats (#43). An agent that names itself gets a seat of
//! its own, a `wl_seat` with a pointer and a keyboard whose focus is its
//! own: its clicks and keys reach its windows while the user's pointer and
//! keyboard stay where they are. The seat goes when the connection does.
//!
//! The agent shows before it acts (idea 27): its pointer is drawn in its
//! colour with its name, a click waits a moment under a ring with the
//! agent's intent in one line, and a sensitive action (typing into a
//! password field, acting in an application of `[agents] confirm_apps`)
//! waits on a card until the user allows it, refuses it, or takes over;
//! past `[agents] confirm_timeout_s` it is refused. Everything waits on the
//! compositor's clock, checked after each dispatch: no work per frame.
//!
//! The user may take over from an agent (in the column, or on the card):
//! its window gets the user's keyboard and the agent's actions are refused
//! until they hand back.
//!
//! An agent works where its windows are (#229): on a desktop no screen
//! shows, a window keeps living as if shown. Its clicks land there, and
//! while an agent drives it the window gets a frame callback now and then,
//! so that its application draws on for the agent's captures; nothing is
//! drawn on a screen for it. Its pointer and ring show once a screen shows
//! that desktop; a card that waits shows wherever the user is. A minimized
//! window an agent acts on comes back on its own desktop, without the
//! user's keyboard.

use std::time::Duration;

use hocus_api::{ActTarget, Action, ApiError, ConnectionId, ErrorCode, JournalEntry, Reply};
use hocus_core::{
    WindowId,
    config::{AgentAccess, McpAgent},
};
use hocus_shell::seat::{Answer, Confirm, SeatView, Showing};
use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    desktop::{Window, WindowSurfaceType},
    input::{
        Seat,
        keyboard::{FilterResult, Keycode, XkbConfig},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    output::Output,
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_server::{backend::GlobalId, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::seat::WaylandFocus as _,
};

use crate::{State, desktops::model_id, shell::Panel, surface::title_and_app_id};

/// Evdev codes are offset by 8 in XKB.
const XKB_OFFSET: u32 = 8;
/// What the agent does stays beside its name this long after it acted.
const SAID_FOR: Duration = Duration::from_secs(2);
/// Its pointer shows this long after it last acted.
const SHOWN_FOR: Duration = Duration::from_secs(10);
/// A seat's global stays this long after the agent went, disabled, so
/// that a client binding it meanwhile is not cut off.
const GLOBAL_LINGER: Duration = Duration::from_secs(5);
/// How long an action waits for the window's application to take the
/// agent's seat, just made, before it is refused.
const BIND_WAIT: Duration = Duration::from_secs(2);
/// How long typing waits once the agent's keyboard came into a window, for
/// its application to say what the field is for (a password?).
const SETTLE: Duration = Duration::from_millis(300);
/// A window no screen shows is driven this long after an agent acted on
/// it: its application gets frame callbacks meanwhile.
const DRIVEN_FOR: Duration = Duration::from_secs(10);
/// How often such a window gets one: enough for an agent's captures to
/// follow, little for its application to draw.
const UNSEEN_FRAME: Duration = Duration::from_millis(100);
/// How long an agent's capture of such a window waits for its application
/// to draw, once woken, before it is taken as it is.
const CAPTURE_WAIT: Duration = Duration::from_millis(200);

impl crate::shell::Pixels for SeatView {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

/// Every agent's seat.
#[derive(Debug, Default)]
pub struct Seats {
    list: Vec<AgentSeat>,
    /// Seats of agents gone, removed once no client can be binding them.
    retired: Vec<(GlobalId, Duration)>,
    /// Tells apart the seats of one agent's connections.
    next: u64,
    /// When the loop was last asked to wake for a seat.
    waking: Option<Duration>,
    /// An agent's click on the shell goes through the user's pointer: the
    /// agents' cards ignore it meanwhile.
    injecting: bool,
    /// When windows agents drive that no screen shows get their next frame
    /// callback, while there are some.
    unseen_frame: Option<Duration>,
    /// Agents' captures of such windows, waiting for them to draw.
    captures: Vec<CaptureWait>,
    /// Windows whose applications were asked to answer once they handled
    /// what an agent's wait expects of them (see [`State::ask_answer`]),
    /// and whether to ask again: one was already asked then.
    asked: Vec<(WindowId, bool)>,
}

/// An agent's capture of a window no screen shows, woken to draw first.
#[derive(Debug, Clone)]
pub(crate) struct CaptureWait {
    pub connection: ConnectionId,
    /// The request waiting for its reply.
    pub id: u64,
    pub window: WindowId,
    pub cursor: bool,
    /// Taken as it is then.
    until: Duration,
    /// Its application drew since.
    drawn: bool,
}

#[derive(Debug)]
struct AgentSeat {
    connection: ConnectionId,
    name: String,
    seat: Seat<State>,
    /// Where its pointer is drawn, global logical, while on a screen.
    at: Option<Point<f64, Logical>>,
    /// The window under it there, if any: while no screen shows that
    /// window, its pointer does not show.
    on: Option<WindowId>,
    /// When it last asked for something.
    last: Duration,
    /// What it does, said beside its name.
    said: Option<String>,
    /// The action it waits on.
    pending: Option<Pending>,
    /// The user took over from it.
    taken: bool,
    /// The window it acts in.
    window: Option<WindowId>,
    /// The window it captured last, and when: driven too, for a while.
    watched: Option<(WindowId, Duration)>,
    /// The configuration its keyboard's layouts come from.
    keymap: Option<u64>,
    /// What it shows, and the key it was made for.
    view: Option<(ViewKey, Panel<SeatView>)>,
}

/// What a seat's view is made of: made again when it changes.
#[derive(Debug, Clone, PartialEq)]
struct ViewKey {
    output: String,
    at: (i32, i32),
    showing: Showing,
    generation: u64,
}

#[derive(Debug)]
struct Pending {
    /// The request waiting for its reply.
    id: u64,
    target: ActTarget,
    action: Action,
    /// What the agent says it does, shown.
    intent: String,
    entry: (u8, u8, JournalEntry),
    phase: Phase,
    /// The rectangle of an application's element it aims at (#175), from
    /// the point: the ring surrounds it.
    around: Option<[i32; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// Until the window's application took the agent's seat (a seat that
    /// just came is bound a moment later), or the time is up.
    Binding { until: Duration },
    /// The agent's keyboard just came into the window: until its
    /// application said what its field is for.
    Settling { until: Duration },
    /// Under the ring, until it lands.
    Showing { until: Duration },
    /// On the card, until the user answers or the time is up; `keys` once
    /// the user clicked it: it has their keyboard.
    Confirming {
        until: Duration,
        why: Sensitive,
        keys: bool,
    },
}

/// Why an action waits for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sensitive {
    /// Typing where the window's application says a password goes.
    Password,
    /// An application of `[agents] confirm_apps`, by its name.
    App(String),
}

/// What an agent's action aims at, checked.
#[derive(Debug)]
struct Aim {
    target: Aimed,
    /// What it does in words, when the agent said nothing.
    words: String,
    sensitive: Option<Sensitive>,
}

#[derive(Debug)]
enum Aimed {
    /// A point of a window shown, global logical.
    Point {
        window: Window,
        at: Point<f64, Logical>,
    },
    /// The window's keyboard.
    Keys { window: Window },
    /// The centre of an element of the shell.
    Shell(Point<f64, Logical>),
}

impl Aim {
    /// The window it acts in, if any.
    fn window(&self) -> Option<&Window> {
        match &self.target {
            Aimed::Point { window, .. } | Aimed::Keys { window } => Some(window),
            Aimed::Shell(_) => None,
        }
    }
}

fn denied(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::PermissionDenied, message)
}

fn taken_over(name: &str) -> ApiError {
    denied(format!(
        "the user took over from agent {name:?}: its actions are refused until they hand back"
    ))
}

impl State {
    /// Connection `connection`, agent `name`, gets a seat of its own.
    pub(crate) fn seat_join(&mut self, connection: ConnectionId, name: &str) {
        // Unique among the seats: a second connection of one agent is told
        // apart by a number.
        let mut seat_name = format!("agent-{name}");
        while self.seats.list.iter().any(|s| s.seat.name() == seat_name) {
            self.seats.next += 1;
            seat_name = format!("agent-{name}-{}", self.seats.next + 1);
        }
        let mut seat = self.seat_state.new_wl_seat(&self.display_handle, seat_name);
        if let Err(err) = seat.add_keyboard(XkbConfig::default(), 200, 25) {
            tracing::warn!(?err, agent = name, "an agent's seat has no keyboard");
        }
        seat.add_pointer();
        tracing::info!(agent = name, "an agent's seat");
        self.seats.list.push(AgentSeat {
            connection,
            name: name.to_owned(),
            seat,
            at: None,
            on: None,
            last: self.clock.now(),
            said: None,
            pending: None,
            taken: false,
            window: None,
            watched: None,
            keymap: None,
            view: None,
        });
        if self.seats.list.len() == 1 {
            self.a11y_agents(true);
        }
    }

    /// The seat of an agent's connection goes with it, and what it waited
    /// for.
    fn seat_leave(&mut self, connection: ConnectionId) {
        let Some(i) = self
            .seats
            .list
            .iter()
            .position(|s| s.connection == connection)
        else {
            return;
        };
        let mut gone = self.seats.list.remove(i);
        if let Some(p) = gone.pending.take() {
            let why = ApiError::new(ErrorCode::Failed, "the agent went away first");
            self.journal_keep(p.entry, Some(&why));
        }
        let serial = SERIAL_COUNTER.next_serial();
        let time = self.time_msec();
        if let Some(keyboard) = gone.seat.get_keyboard() {
            use smithay::wayland::text_input::TextInputSeat as _;
            gone.seat.text_input().leave();
            keyboard.set_focus(self, None, serial);
        }
        if let Some(pointer) = gone.seat.get_pointer() {
            let location = pointer.current_location();
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location,
                    serial,
                    time,
                },
            );
            pointer.frame(self);
        }
        gone.seat.remove_keyboard();
        gone.seat.remove_pointer();
        if let Some(global) = gone.seat.global() {
            self.display_handle.disable_global::<State>(global.clone());
            self.seats
                .retired
                .push((global, self.clock.now() + GLOBAL_LINGER));
        }
        tracing::info!(agent = gone.name, "an agent's seat went");
        if self.seats.list.is_empty() {
            self.a11y_agents(false);
        }
        self.needs_redraw = true;
    }

    /// Whether this `wl_seat` is an agent's.
    pub(crate) fn is_agent_wl_seat(
        &self,
        seat: &smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
    ) -> bool {
        self.seats.list.iter().any(|s| s.seat.owns(seat))
    }

    fn agent_seat(&mut self, connection: ConnectionId) -> Option<&mut AgentSeat> {
        self.seats
            .list
            .iter_mut()
            .find(|s| s.connection == connection)
    }

    /// An agent's action (#43): checked, shown, confirmed when sensitive,
    /// then carried out through its own seat. `None` while it waits: the
    /// reply follows.
    pub(crate) fn agent_act(
        &mut self,
        connection: ConnectionId,
        id: u64,
        name: &str,
        target: ActTarget,
        action: Action,
        intent: Option<String>,
    ) -> Option<Result<Reply, ApiError>> {
        // The window as it is before the action changes it.
        let entry = self.journal_entry(name, &target, &action);
        self.agent_act_aimed(connection, id, name, target, action, intent, entry, None)
    }

    /// The same, its journal entry made, and the element of an
    /// application's tree it aims at when it does (#175), found.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn agent_act_aimed(
        &mut self,
        connection: ConnectionId,
        id: u64,
        name: &str,
        target: ActTarget,
        action: Action,
        intent: Option<String>,
        entry: (u8, u8, JournalEntry),
        found: Option<crate::a11y::Found>,
    ) -> Option<Result<Reply, ApiError>> {
        // An element being looked for is an action under way too.
        if self.resolving(connection)
            || self
                .agent_seat(connection)
                .is_some_and(|s| s.pending.is_some())
        {
            let busy = ApiError::new(
                ErrorCode::BadRequest,
                "one action at a time: the last one is not done yet",
            );
            self.journal_keep(entry, Some(&busy));
            return Some(Err(busy));
        }
        let mut aim = match self.agent_aim(connection, name, &target, &action) {
            Ok(aim) => aim,
            Err(err) => {
                self.journal_keep(entry, Some(&err));
                return Some(Err(err));
            }
        };
        // A minimized window comes back on its desktop, then is aimed at
        // again where it is now.
        if let Some(id) = aim.window().and_then(model_id)
            && self.model.is_minimized(id)
        {
            self.agent_restore(id, name);
            aim = match self.agent_aim(connection, name, &target, &action) {
                Ok(aim) => aim,
                Err(err) => {
                    self.journal_keep(entry, Some(&err));
                    return Some(Err(err));
                }
            };
        }
        let now = self.clock.now();
        let lang = self.shell_lang();
        let words = intent.unwrap_or_else(|| match (&found, &action) {
            (Some(found), Action::Scroll { .. }) => lang.scrolls_in(&format!("“{}”", found.label)),
            (Some(found), _) => lang.clicks_on(&found.label),
            (None, _) => aim.words.clone(),
        });
        let phase = self.first_phase(connection, &aim, &action, now);
        let shown = self.shown_point(&aim);
        if let Some(seat) = self.agent_seat(connection) {
            seat.last = now;
            seat.said = Some(words.clone());
            if let Some((at, on)) = shown {
                seat.at = Some(at);
                seat.on = on;
            }
            if let Some(window) = aim.window() {
                seat.window = model_id(window);
            }
        }
        self.needs_redraw = true;
        match phase {
            Some(phase) => {
                if let Phase::Confirming { why, .. } = &phase {
                    tracing::info!(agent = name, ?why, "an agent's action waits for the user");
                }
                if let Some(seat) = self.agent_seat(connection) {
                    seat.pending = Some(Pending {
                        id,
                        target,
                        action,
                        intent: words,
                        entry,
                        phase,
                        around: found.map(|f| f.around),
                    });
                }
                self.wake_for_seats();
                None
            }
            None => {
                let result = self.agent_carry_out(connection, &aim, &action);
                self.journal_keep(entry, result.as_ref().err());
                Some(result)
            }
        }
    }

    /// Whether agent `name` may do `action` in `window`, as the
    /// configuration in effect says: before an element of its tree is
    /// looked for.
    pub(crate) fn agent_may(
        &mut self,
        connection: ConnectionId,
        name: &str,
        window: u64,
        action: &Action,
    ) -> Result<(), ApiError> {
        self.agent_aim(
            connection,
            name,
            &ActTarget::Window { window, at: None },
            action,
        )
        .map(drop)
    }

    /// Whether the agent of this connection waits on an action.
    pub(crate) fn seat_busy(&self, connection: ConnectionId) -> bool {
        self.seats
            .list
            .iter()
            .any(|s| s.connection == connection && s.pending.is_some())
    }

    /// Brings back a minimized window an agent acts on (#229): on its own
    /// desktop, shown there or not, and without the user's keyboard, which
    /// stays where it is.
    fn agent_restore(&mut self, id: WindowId, name: &str) {
        if let Err(err) = self.model.unminimize(id) {
            tracing::warn!(%err, "an agent's window stays minimized");
            return;
        }
        tracing::info!(
            agent = name,
            window = id.0,
            "an agent brings its window back"
        );
        self.sync_desktops();
    }

    /// What an action waits on first: its window's application taking the
    /// seat, then, for typing in a window the agent's keyboard just came
    /// into, the application saying what its field is for, then the rest.
    fn first_phase(
        &mut self,
        connection: ConnectionId,
        aim: &Aim,
        action: &Action,
        now: Duration,
    ) -> Option<Phase> {
        if !self.seat_reaches(connection, aim, action) {
            return Some(Phase::Binding {
                until: now + BIND_WAIT,
            });
        }
        if let Aimed::Keys { window } = &aim.target
            && self.agent_focus(connection, window)
        {
            if let Some(id) = model_id(window) {
                self.ask_answer(id);
            }
            return Some(Phase::Settling {
                until: now + SETTLE,
            });
        }
        self.next_phase(aim, action, now)
    }

    /// Gives `window` the agent's keyboard, its text inputs told (no input
    /// method runs, so smithay tells them only for this seat, here): its
    /// application says then what its field is for. Whether it came now.
    fn agent_focus(&mut self, connection: ConnectionId, window: &Window) -> bool {
        use smithay::wayland::text_input::TextInputSeat as _;
        let (Some(seat), Some(surface)) = (
            self.agent_seat(connection).map(|s| s.seat.clone()),
            window.wl_surface().map(|s| s.into_owned()),
        ) else {
            return false;
        };
        let Some(keyboard) = seat.get_keyboard() else {
            return false;
        };
        if keyboard.current_focus().as_ref() == Some(&surface) {
            return false;
        }
        self.agent_keyboard(connection, &seat);
        seat.text_input().leave();
        keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
        seat.text_input().enter();
        true
    }

    /// What an action waits on once its window's application has the seat:
    /// the user, for a sensitive one; the ring, for a click.
    fn next_phase(&self, aim: &Aim, action: &Action, now: Duration) -> Option<Phase> {
        let delay = self.config.agents.show_before_click();
        match &aim.sensitive {
            Some(why) => Some(Phase::Confirming {
                until: now + self.config.agents.confirm_timeout(),
                why: why.clone(),
                keys: false,
            }),
            None if matches!(action, Action::Click { .. }) && !delay.is_zero() => {
                Some(Phase::Showing { until: now + delay })
            }
            None => None,
        }
    }

    /// Whether the window's application took the agent's seat: its
    /// pointer, or its keyboard for typing. Most applications take every
    /// seat; one that takes the first alone never gets the agent's input.
    fn seat_reaches(&self, connection: ConnectionId, aim: &Aim, action: &Action) -> bool {
        use smithay::reexports::wayland_server::Resource as _;
        let window = match &aim.target {
            Aimed::Shell(_) => return true,
            Aimed::Point { window, .. } | Aimed::Keys { window } => window,
        };
        let Some(seat) = self
            .seats
            .list
            .iter()
            .find(|s| s.connection == connection)
            .map(|s| &s.seat)
        else {
            return false;
        };
        let Some(client) = window.wl_surface().and_then(|s| s.client()) else {
            return false;
        };
        match action {
            Action::Type { .. } | Action::Keys { .. } => seat
                .get_keyboard()
                .is_some_and(|k| k.client_keyboards(&client).next().is_some()),
            Action::Click { .. } | Action::Scroll { .. } => seat
                .get_pointer()
                .is_some_and(|p| p.client_pointers(&client).next().is_some()),
        }
    }

    /// Where the agent's pointer shows for an action, and the window under
    /// it: the point, or the middle of the window it types in, when the
    /// user would see it there.
    fn shown_point(&self, aim: &Aim) -> Option<(Point<f64, Logical>, Option<WindowId>)> {
        match &aim.target {
            Aimed::Point { at, window } => Some((*at, model_id(window))),
            Aimed::Shell(at) => Some((*at, None)),
            Aimed::Keys { window } => {
                let g = self.agent_geometry(window)?;
                let centre = g.loc.to_f64()
                    + Point::from((f64::from(g.size.w) / 2.0, f64::from(g.size.h) / 2.0));
                // On a screen, not over another window; on a desktop no
                // screen shows, where it shows once one does.
                let shown = self.space.element_location(window).is_some();
                let over = self
                    .space
                    .element_under(centre)
                    .is_some_and(|(w, _)| w == window);
                (!shown || over).then(|| (centre, model_id(window)))
            }
        }
    }

    /// Where a window is in the global space: where it is mapped, or, on a
    /// desktop no screen shows (#229), where it goes once one does.
    fn agent_geometry(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        let at = self.space.element_location(window).or_else(|| {
            let id = model_id(window)?;
            Some(self.windows.get(&id)?.location)
        })?;
        Some(Rectangle::new(at, window.geometry().size))
    }

    /// Checks what agent `name` asks for, as the configuration in effect
    /// says, and finds what it aims at.
    fn agent_aim(
        &mut self,
        connection: ConnectionId,
        name: &str,
        target: &ActTarget,
        action: &Action,
    ) -> Result<Aim, ApiError> {
        let agent: McpAgent = self
            .config
            .agents
            .mcp_agent(name)
            .cloned()
            .ok_or_else(|| denied(format!("agent {name:?} is no longer in [[agents.mcp]]")))?;
        if agent.access != AgentAccess::Act {
            return Err(denied(format!(
                "agent {name:?} may only read: its access is \"read\" in [[agents.mcp]]"
            )));
        }
        if self.locked() {
            return Err(denied("the session is locked: agents do not act on it"));
        }
        if self.auth_open() {
            return Err(denied(
                "a password dialog is open: agents do not act until it is answered",
            ));
        }
        if self
            .seats
            .list
            .iter()
            .any(|s| s.connection == connection && s.taken)
        {
            return Err(taken_over(name));
        }
        let lang = self.shell_lang();
        let typing = matches!(action, Action::Type { .. } | Action::Keys { .. });
        // What is typed stays out of the log: it may be a password.
        tracing::debug!(agent = name, ?target, typing, "an agent acts");
        match target {
            ActTarget::AppElement { .. } => Err(ApiError::new(
                ErrorCode::BadRequest,
                "an application's element is found before it is aimed at",
            )),
            ActTarget::Element { label } => {
                if typing {
                    return Err(ApiError::new(
                        ErrorCode::BadRequest,
                        "typing goes to a window: aim at one",
                    ));
                }
                if !agent.shell {
                    return Err(denied(format!(
                        "agent {name:?} may not use the shell's interface: shell = true in \
                         [[agents.mcp]] allows it"
                    )));
                }
                let at = self.element_centre(label)?;
                let words = match action {
                    Action::Scroll { .. } => lang.scrolls_in(&format!("“{label}”")),
                    _ => lang.clicks_on(label),
                };
                Ok(Aim {
                    target: Aimed::Shell(at),
                    words,
                    sensitive: None,
                })
            }
            ActTarget::Window { window: id, at } => {
                let window = self
                    .windows
                    .get(&WindowId(*id))
                    .map(|m| m.window.clone())
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::NotFound, format!("no window with id {id}"))
                    })?;
                let (title, app_id) = title_and_app_id(&window);
                let app_id = app_id.unwrap_or_default();
                if !agent.may_act_on(&app_id) {
                    return Err(denied(format!(
                        "agent {name:?} may not act on windows of {app_id:?}: its apps in \
                         [[agents.mcp]] do not match it"
                    )));
                }
                if window.x11_surface().is_some() {
                    return Err(denied(
                        "an X11 window (XWayland) has the session's pointer and keyboard \
                         alone: agents act on Wayland windows",
                    ));
                }
                let what = title
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| app_id.clone());
                let app = self.app_name(&app_id);
                let sensitive = if self.config.agents.confirms(&app_id)
                    && !matches!(action, Action::Scroll { .. })
                {
                    Some(Sensitive::App(app))
                } else if typing && self.password_field_in(&window) {
                    Some(Sensitive::Password)
                } else {
                    None
                };
                let (target, words) = match action {
                    Action::Type { .. } => (Aimed::Keys { window }, lang.types_in(&what)),
                    Action::Keys { keys } => (Aimed::Keys { window }, lang.presses_in(keys, &what)),
                    Action::Click { .. } => {
                        let at = self.agent_point(&window, *id, *at)?;
                        (Aimed::Point { window, at }, lang.clicks_in(&what))
                    }
                    Action::Scroll { .. } => {
                        let at = self.agent_point(&window, *id, *at)?;
                        (Aimed::Point { window, at }, lang.scrolls_in(&what))
                    }
                };
                Ok(Aim {
                    target,
                    words,
                    sensitive,
                })
            }
        }
    }

    /// An application's name as the launcher knows it, else its app id.
    fn app_name(&self, app_id: &str) -> String {
        self.launcher
            .as_ref()
            .and_then(|l| l.app_for(app_id))
            .map_or_else(|| app_id.to_owned(), |a| a.name.clone())
    }

    /// A point of `window` (from its top-left corner; its centre when
    /// `None`) in the global space, wherever the window is: on a screen,
    /// or on a desktop no screen shows (#229). Nothing comes forward.
    fn agent_point(
        &self,
        window: &Window,
        id: u64,
        at: Option<hocus_api::Point>,
    ) -> Result<Point<f64, Logical>, ApiError> {
        let geometry = self.agent_geometry(window).ok_or_else(|| {
            ApiError::new(
                ErrorCode::Failed,
                format!("window {id} has no place: it is not managed"),
            )
        })?;
        let (w, h) = (f64::from(geometry.size.w), f64::from(geometry.size.h));
        let (x, y) = at.map_or((w / 2.0, h / 2.0), |p| (p.x, p.y));
        if !(0.0..w).contains(&x) || !(0.0..h).contains(&y) {
            return Err(ApiError::new(
                ErrorCode::BadRequest,
                format!("{x},{y} is outside the window, which is {w}×{h}"),
            ));
        }
        Ok(geometry.loc.to_f64() + Point::from((x, y)))
    }

    /// Carries out a checked action through the agent's own seat; a click
    /// on the shell through the user's pointer, put back after.
    fn agent_carry_out(
        &mut self,
        connection: ConnectionId,
        aim: &Aim,
        action: &Action,
    ) -> Result<Reply, ApiError> {
        let Some(seat) = self.agent_seat(connection).map(|s| s.seat.clone()) else {
            return Err(ApiError::new(ErrorCode::Failed, "the agent has no seat"));
        };
        let time = self.time_msec();
        match &aim.target {
            Aimed::Shell(at) => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return Err(ApiError::new(ErrorCode::Failed, "no pointer"));
                };
                let back = pointer.current_location();
                self.seats.injecting = true;
                self.pointer_motion(*at, time);
                match action {
                    Action::Click { button, count } => {
                        for _ in 0..(*count).clamp(1, 3) {
                            self.pointer_button(button.code(), ButtonState::Pressed, time);
                            self.pointer_button(button.code(), ButtonState::Released, time);
                        }
                    }
                    Action::Scroll { dy } => self.scroll(*dy, time),
                    Action::Type { .. } | Action::Keys { .. } => {}
                }
                self.pointer_motion(back, time);
                self.seats.injecting = false;
            }
            Aimed::Point { window, at } => {
                let pointer = seat
                    .get_pointer()
                    .ok_or_else(|| ApiError::new(ErrorCode::Failed, "the agent has no pointer"))?;
                // Whatever is over it, the point is the window's: the
                // seat is the agent's alone. Its surfaces start where its
                // geometry says, on a screen or not.
                let geometry = self.agent_geometry(window).unwrap_or_default();
                let origin = geometry.loc - window.geometry().loc;
                let focus = window
                    .surface_under(*at - origin.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + origin).to_f64()));
                pointer.motion(
                    self,
                    focus,
                    &MotionEvent {
                        location: *at,
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
                pointer.frame(self);
                // The agent's keyboard follows its clicks, as the user's
                // does theirs.
                if matches!(action, Action::Click { .. }) {
                    self.agent_focus(connection, window);
                }
                match action {
                    Action::Click { button, count } => {
                        for _ in 0..(*count).clamp(1, 3) {
                            for state in [ButtonState::Pressed, ButtonState::Released] {
                                pointer.button(
                                    self,
                                    &ButtonEvent {
                                        button: button.code(),
                                        state,
                                        serial: SERIAL_COUNTER.next_serial(),
                                        time,
                                    },
                                );
                                pointer.frame(self);
                            }
                        }
                    }
                    Action::Scroll { dy } => {
                        let frame = AxisFrame::new(time)
                            .source(AxisSource::Wheel)
                            .value(Axis::Vertical, *dy)
                            .v120(Axis::Vertical, (dy / 15.0 * 120.0) as i32);
                        pointer.axis(self, frame);
                        pointer.frame(self);
                    }
                    Action::Type { .. } | Action::Keys { .. } => {}
                }
            }
            Aimed::Keys { window } => {
                let surface = window
                    .wl_surface()
                    .map(|s| s.into_owned())
                    .ok_or_else(|| ApiError::new(ErrorCode::Failed, "the window has no surface"))?;
                self.agent_keyboard(connection, &seat);
                let chords = self.agent_chords(&seat, action)?;
                self.agent_type(&seat, &surface, &chords);
            }
        }
        Ok(Reply::Done)
    }

    /// The agent's keyboard has the user's layouts, the one in use too.
    fn agent_keyboard(&mut self, connection: ConnectionId, seat: &Seat<State>) {
        let Some(keyboard) = seat.get_keyboard() else {
            return;
        };
        let generation = self.config_generation;
        if self.agent_seat(connection).and_then(|s| s.keymap) != Some(generation) {
            let k = self.config.keyboard.clone();
            let (layout, variant) = hocus_core::keyboard::xkb_lists(&k.layouts);
            let config = XkbConfig {
                layout: &layout,
                variant: &variant,
                options: (!k.options.is_empty()).then(|| k.options.clone()),
                ..Default::default()
            };
            if let Err(err) = keyboard.set_xkb_config(self, config) {
                tracing::warn!(?err, "an agent's keyboard keeps its layouts");
            }
            if let Some(s) = self.agent_seat(connection) {
                s.keymap = Some(generation);
            }
        }
        let index = self.layout_index as u32;
        keyboard.with_xkb_state(self, |mut ctx| {
            ctx.set_layout(smithay::input::keyboard::Layout(index));
        });
    }

    /// The keys an action presses, each chord modifiers first, looked up
    /// before any is pressed: nothing half-typed.
    fn agent_chords(
        &mut self,
        seat: &Seat<State>,
        action: &Action,
    ) -> Result<Vec<Vec<u32>>, ApiError> {
        let keyboard = seat
            .get_keyboard()
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "the agent has no keyboard"))?;
        match action {
            Action::Type { text } => self.text_chords(&keyboard, text),
            Action::Keys { keys } => Ok(vec![self.chord_on(&keyboard, keys)?]),
            Action::Click { .. } | Action::Scroll { .. } => Ok(Vec::new()),
        }
    }

    /// Gives `surface` the agent's keyboard, and presses the chords there.
    fn agent_type(&mut self, seat: &Seat<State>, surface: &WlSurface, chords: &[Vec<u32>]) {
        let Some(keyboard) = seat.get_keyboard() else {
            return;
        };
        if keyboard.current_focus().as_ref() != Some(surface) {
            use smithay::wayland::text_input::TextInputSeat as _;
            seat.text_input().leave();
            keyboard.set_focus(self, Some(surface.clone()), SERIAL_COUNTER.next_serial());
            seat.text_input().enter();
        }
        let time = self.time_msec();
        for chord in chords {
            let presses = chord
                .iter()
                .map(|c| (*c, KeyState::Pressed))
                .chain(chord.iter().rev().map(|c| (*c, KeyState::Released)));
            for (code, state) in presses {
                keyboard.input::<(), _>(
                    self,
                    Keycode::new(code + XKB_OFFSET),
                    state,
                    SERIAL_COUNTER.next_serial(),
                    time,
                    |_, _, _| FilterResult::Forward,
                );
            }
        }
    }

    /// After each dispatch: seats of agents gone go, actions due land,
    /// confirmations past their time are refused, pointers of idle agents
    /// hide, and what shows is made again where it changed.
    pub fn refresh_seats(&mut self) {
        for connection in self.api.take_departed() {
            self.seat_leave(connection);
        }
        if self.seats.list.is_empty() && self.seats.retired.is_empty() {
            return;
        }
        let now = self.clock.now();
        let (removed, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.seats.retired)
            .into_iter()
            .partition(|(_, at)| now >= *at);
        self.seats.retired = kept;
        for (global, _) in removed {
            self.display_handle.remove_global::<State>(global);
        }
        self.refresh_binding(now);
        self.frame_unseen(now);
        let due: Vec<ConnectionId> = self
            .seats
            .list
            .iter()
            .filter(|s| {
                s.pending.as_ref().is_some_and(|p| match p.phase {
                    Phase::Settling { until }
                    | Phase::Showing { until }
                    | Phase::Confirming { until, .. } => now >= until,
                    Phase::Binding { .. } => false,
                })
            })
            .map(|s| s.connection)
            .collect();
        for connection in due {
            let Some(pending) = self.agent_seat(connection).and_then(|s| s.pending.take()) else {
                continue;
            };
            match pending.phase {
                Phase::Binding { .. } => {}
                Phase::Settling { .. } => self.go_on(connection, pending, now),
                Phase::Showing { .. } => self.land_pending(connection, pending, false),
                Phase::Confirming { .. } => {
                    let s = self.config.agents.confirm_timeout().as_secs();
                    let err = denied(format!(
                        "the user did not confirm it within {s} s: nothing was done"
                    ));
                    self.journal_keep(pending.entry, Some(&err));
                    self.api.reply(connection, pending.id, Err(err));
                    self.needs_redraw = true;
                }
            }
        }
        self.redraw_seats();
        self.wake_for_seats();
    }

    /// Windows agents drive that no screen shows get a frame callback every
    /// [`UNSEEN_FRAME`] (#229), so that their applications draw what the
    /// agents do, for their captures. Nothing is drawn for them, and
    /// nothing wakes once no agent drives one.
    fn frame_unseen(&mut self, now: Duration) {
        let driven = self.unseen_driven(now);
        if driven.is_empty() {
            self.seats.unseen_frame = None;
            return;
        }
        if self.seats.unseen_frame.is_some_and(|at| now < at) {
            return;
        }
        for (window, output) in &driven {
            window.send_frame(output, now, None, |_, _| Some(output.clone()));
        }
        self.seats.unseen_frame = Some(now + UNSEEN_FRAME);
    }

    /// The windows agents drive (acting on them, or lately) that no screen
    /// shows, each with the output that shows its desktop once shown.
    fn unseen_driven(&self, now: Duration) -> Vec<(Window, Output)> {
        let mut out: Vec<(Window, Output)> = Vec::new();
        let driven = self.seats.list.iter().filter(|s| !s.taken).flat_map(|s| {
            let acting = s.pending.is_some() || now.saturating_sub(s.last) < DRIVEN_FOR;
            let watching = s
                .watched
                .filter(|(_, at)| now.saturating_sub(*at) < DRIVEN_FOR)
                .map(|(id, _)| id);
            [s.window.filter(|_| acting), watching]
        });
        for id in driven.flatten() {
            let Some(managed) = self.windows.get(&id) else {
                continue;
            };
            if self.space.element_location(&managed.window).is_some()
                || out.iter().any(|(w, _)| *w == managed.window)
            {
                continue;
            }
            if let Some(output) = self.host_output(id) {
                out.push((managed.window.clone(), output));
            }
        }
        out
    }

    /// The output that shows a window's desktop, or would: its host.
    fn host_output(&self, id: WindowId) -> Option<Output> {
        self.model
            .desktop_of(id)
            .and_then(|d| self.model.host_of(d))
            .and_then(|o| self.find_output(&o.name.0))
            .or_else(|| self.space.outputs().next().cloned())
    }

    /// An agent captures window `id` (#229): it drives it for a while, as
    /// an action does. Where no screen shows it and its application waits
    /// for a frame callback, it gets one now, and the capture waits for it
    /// to draw (at most [`CAPTURE_WAIT`]): whether it waits.
    pub(crate) fn agent_watch(&mut self, connection: ConnectionId, id: WindowId) -> bool {
        let now = self.clock.now();
        let Some(seat) = self.agent_seat(connection) else {
            return false;
        };
        seat.watched = Some((id, now));
        let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) else {
            return false;
        };
        if self.space.element_location(&window).is_some() {
            return false;
        }
        let Some(output) = self.host_output(id) else {
            return false;
        };
        let mut asked = false;
        window.with_surfaces(|_, data| {
            use smithay::wayland::compositor::SurfaceAttributes;
            asked |= !data
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .is_empty();
        });
        window.send_frame(&output, now, None, |_, _| Some(output.clone()));
        self.wake_for_seats();
        asked
    }

    /// A capture waits for its window to draw.
    pub(crate) fn capture_wait(
        &mut self,
        connection: ConnectionId,
        id: u64,
        window: WindowId,
        cursor: bool,
    ) {
        self.seats.captures.push(CaptureWait {
            connection,
            id,
            window,
            cursor,
            until: self.clock.now() + CAPTURE_WAIT,
            drawn: false,
        });
        self.ask_answer(window);
        self.wake_for_seats();
    }

    /// Asks window `id`'s application to answer (`xdg_wm_base.ping`) once
    /// it handled what it was just sent: a frame callback for a capture,
    /// the agent's keyboard before typing. Its answer comes after what it
    /// did about them on its connection, a commit or a field's purpose.
    /// A session still waits its time, since an application may draw from
    /// another thread after it answered; on a virtual clock, the test
    /// harness moves the clock past such a wait once the application
    /// answered, however long its thread took (#235).
    pub(crate) fn ask_answer(&mut self, id: WindowId) {
        use smithay::wayland::shell::PingError;
        let windows = &self.windows;
        self.seats.asked.retain(|(id, _)| windows.contains_key(id));
        let Some(toplevel) = self.windows.get(&id).and_then(|m| m.window.toplevel()) else {
            return;
        };
        let again = match toplevel.client().send_ping(SERIAL_COUNTER.next_serial()) {
            Ok(()) => false,
            // Its answer to the one before does not say it handled this.
            Err(PingError::PingAlreadyPending(_)) => true,
            Err(PingError::DeadSurface) => return,
        };
        self.seats.asked.push((id, again));
    }

    /// An application answered: what it was asked about before is
    /// handled, and it is asked again for what came after.
    pub(crate) fn app_answered(&mut self, client: &smithay::wayland::shell::xdg::ShellClient) {
        if self.seats.asked.is_empty() {
            return;
        }
        let windows = &self.windows;
        let of_client = |id: &WindowId| {
            windows
                .get(id)
                .and_then(|m| m.window.toplevel())
                .is_some_and(|t| t.client() == *client)
        };
        let (answered, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.seats.asked)
            .into_iter()
            .partition(|(id, _)| of_client(id));
        self.seats.asked = kept;
        let again: Vec<WindowId> = answered
            .into_iter()
            .filter_map(|(id, again)| again.then_some(id))
            .collect();
        if let Some(&first) = again.first() {
            self.ask_answer(first);
            // One answer for all of its windows.
            self.seats
                .asked
                .extend(again.iter().skip(1).map(|id| (*id, false)));
        }
    }

    /// Whether every application asked to answer did, or went.
    pub fn apps_answered(&self) -> bool {
        self.seats
            .asked
            .iter()
            .all(|(id, _)| !self.windows.contains_key(id))
    }

    /// A surface committed: a capture waiting on its window goes on.
    pub(crate) fn capture_committed(&mut self, root: &WlSurface) {
        if self.seats.captures.is_empty() {
            return;
        }
        let Some(id) = self.managed_id(root) else {
            return;
        };
        for wait in &mut self.seats.captures {
            if wait.window == id {
                wait.drawn = true;
            }
        }
    }

    /// The captures that may be taken now: their window drew, went, or
    /// the wait is over.
    pub(crate) fn captures_due(&mut self) -> Vec<CaptureWait> {
        if self.seats.captures.is_empty() {
            return Vec::new();
        }
        let now = self.clock.now();
        let windows = &self.windows;
        let (due, kept) = std::mem::take(&mut self.seats.captures)
            .into_iter()
            .partition(|w| w.drawn || now >= w.until || !windows.contains_key(&w.window));
        self.seats.captures = kept;
        due
    }

    /// When the earliest capture waiting for its window is taken anyway,
    /// for the test harness.
    pub fn capture_deadline(&self) -> Option<Duration> {
        self.seats.captures.iter().map(|w| w.until).min()
    }

    /// A waiting action goes on to what it waits on next, checked again
    /// (sensitive now that its field is known?), or lands.
    fn go_on(&mut self, connection: ConnectionId, mut pending: Pending, now: Duration) {
        let Some(name) = self.api.agent(connection).map(str::to_owned) else {
            return;
        };
        let aim = match self.agent_aim(connection, &name, &pending.target, &pending.action) {
            Ok(aim) => aim,
            Err(err) => {
                self.journal_keep(pending.entry, Some(&err));
                self.api.reply(connection, pending.id, Err(err));
                return;
            }
        };
        match self.first_phase(connection, &aim, &pending.action, now) {
            Some(phase) => {
                pending.phase = phase;
                if let Some(seat) = self.agent_seat(connection) {
                    seat.pending = Some(pending);
                }
            }
            None => self.land_pending(connection, pending, false),
        }
        self.needs_redraw = true;
    }

    /// Actions waiting for their window's application to take the seat go
    /// on once it did, and are refused when the time is up.
    fn refresh_binding(&mut self, now: Duration) {
        let binding: Vec<ConnectionId> = self
            .seats
            .list
            .iter()
            .filter(|s| {
                s.pending
                    .as_ref()
                    .is_some_and(|p| matches!(p.phase, Phase::Binding { .. }))
            })
            .map(|s| s.connection)
            .collect();
        for connection in binding {
            let Some(name) = self.api.agent(connection).map(str::to_owned) else {
                continue;
            };
            let Some(pending) = self.agent_seat(connection).and_then(|s| s.pending.take()) else {
                continue;
            };
            let Phase::Binding { until } = pending.phase else {
                continue;
            };
            let aim = match self.agent_aim(connection, &name, &pending.target, &pending.action) {
                Ok(aim) => aim,
                Err(err) => {
                    self.journal_keep(pending.entry, Some(&err));
                    self.api.reply(connection, pending.id, Err(err));
                    continue;
                }
            };
            if self.seat_reaches(connection, &aim, &pending.action) {
                self.go_on(connection, pending, now);
            } else if now >= until {
                let err = ApiError::new(
                    ErrorCode::Failed,
                    "the window's application did not take the agent's seat: it handles the \
                     first seat alone, and only the user's input reaches it",
                );
                self.journal_keep(pending.entry, Some(&err));
                self.api.reply(connection, pending.id, Err(err));
            } else if let Some(seat) = self.agent_seat(connection) {
                seat.pending = Some(pending);
            }
            self.needs_redraw = true;
        }
    }

    /// A waiting action lands now: checked again, since the configuration,
    /// the lock or the user may have changed meanwhile.
    fn land_pending(&mut self, connection: ConnectionId, pending: Pending, confirmed: bool) {
        let Some(name) = self.api.agent(connection).map(str::to_owned) else {
            return;
        };
        let result = self
            .agent_aim(connection, &name, &pending.target, &pending.action)
            .and_then(|aim| self.agent_carry_out(connection, &aim, &pending.action));
        let (hour, minute, mut entry) = pending.entry;
        entry.confirmed = confirmed && result.is_ok();
        self.journal_keep((hour, minute, entry), result.as_ref().err());
        let now = self.clock.now();
        if let Some(seat) = self.agent_seat(connection) {
            seat.last = now;
        }
        self.api.reply(connection, pending.id, result);
        self.needs_redraw = true;
    }

    /// The user answered a card.
    fn seat_answer(&mut self, connection: ConnectionId, answer: Answer) {
        let Some(pending) = self.agent_seat(connection).and_then(|s| s.pending.take()) else {
            return;
        };
        let name = self.api.agent(connection).unwrap_or_default().to_owned();
        tracing::info!(agent = name, ?answer, "the user answered an agent");
        match answer {
            Answer::Allow => self.land_pending(connection, pending, true),
            Answer::Refuse => {
                let err = denied("the user refused it: nothing was done");
                self.journal_keep(pending.entry, Some(&err));
                self.api.reply(connection, pending.id, Err(err));
            }
            Answer::TakeOver => {
                let err = taken_over(&name);
                self.journal_keep(pending.entry, Some(&err));
                self.api.reply(connection, pending.id, Err(err));
                self.take_over(&name);
            }
        }
        self.needs_redraw = true;
    }

    /// Every agent stopped: what they wait on is refused with `why`.
    pub(crate) fn refuse_pending(&mut self, why: &ApiError) {
        let waiting: Vec<(ConnectionId, Pending)> = self
            .seats
            .list
            .iter_mut()
            .filter_map(|s| Some((s.connection, s.pending.take()?)))
            .collect();
        for (connection, pending) in waiting {
            self.journal_keep(pending.entry, Some(why));
            self.api.reply(connection, pending.id, Err(why.clone()));
        }
        self.needs_redraw = true;
    }

    /// The user takes over from agent `name`: its window gets their
    /// keyboard, comes forward, and its actions are refused until they
    /// hand back.
    pub(crate) fn take_over(&mut self, name: &str) {
        let refusal = taken_over(name);
        let mut window = None;
        let mut waiting = Vec::new();
        for seat in self.seats.list.iter_mut().filter(|s| s.name == name) {
            seat.taken = true;
            window = window.or(seat.window);
            if let Some(p) = seat.pending.take() {
                waiting.push((seat.connection, p));
            }
        }
        for (connection, pending) in waiting {
            self.journal_keep(pending.entry, Some(&refusal));
            self.api.reply(connection, pending.id, Err(refusal.clone()));
        }
        // Its keyboard lets go: the window has one keyboard, the user's.
        let seats: Vec<Seat<State>> = self
            .seats
            .list
            .iter()
            .filter(|s| s.name == name)
            .map(|s| s.seat.clone())
            .collect();
        for seat in seats {
            if let Some(keyboard) = seat.get_keyboard() {
                use smithay::wayland::text_input::TextInputSeat as _;
                seat.text_input().leave();
                keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
            }
        }
        if let Some(id) = window {
            self.restore(id, SERIAL_COUNTER.next_serial());
        }
        tracing::info!(agent = name, "the user took over");
        self.needs_redraw = true;
    }

    /// And hands back.
    pub(crate) fn hand_back(&mut self, name: &str) {
        for seat in self.seats.list.iter_mut().filter(|s| s.name == name) {
            seat.taken = false;
            seat.last = self.clock.now();
        }
        tracing::info!(agent = name, "the user handed back");
        self.needs_redraw = true;
    }

    /// The agents connected now, for the column's journal.
    pub(crate) fn seat_lines(&self) -> Vec<hocus_shell::journal::SeatLine> {
        self.seats
            .list
            .iter()
            .map(|s| hocus_shell::journal::SeatLine {
                agent: s.name.clone(),
                window: s.window.and_then(|id| {
                    let w = &self.windows.get(&id)?.window;
                    let (title, app_id) = title_and_app_id(w);
                    title.filter(|t| !t.is_empty()).or(app_id)
                }),
                taken: s.taken,
            })
            .collect()
    }

    /// The window each agent acts in, for the column to show live.
    pub(crate) fn seat_windows(&self) -> Vec<(String, Window)> {
        self.seats
            .list
            .iter()
            .filter_map(|s| {
                let window = self.windows.get(&s.window?)?.window.clone();
                Some((s.name.clone(), window))
            })
            .collect()
    }

    /// What each seat shows now, and where: its pointer while it acts or
    /// acted lately, where a screen shows the window it points at; else its
    /// card alone, while one waits for the user.
    fn seat_showing(
        &self,
        seat: &AgentSeat,
        now: Duration,
    ) -> Option<(Output, Point<f64, Logical>, Showing)> {
        let recent = now.saturating_sub(seat.last) < SHOWN_FOR;
        if !recent && seat.pending.is_none() {
            return None;
        }
        let seen = seat.on.is_none_or(|id| {
            self.windows
                .get(&id)
                .is_some_and(|m| self.space.element_location(&m.window).is_some())
        });
        let asking = seat
            .pending
            .as_ref()
            .is_some_and(|p| matches!(p.phase, Phase::Confirming { .. }));
        if !seen && !asking {
            return None;
        }
        let mut at = seat.at?;
        let output = match self.space.output_under(at).next() {
            Some(output) => output.clone(),
            // Where no screen is now (its screen went): the card shows on
            // the first, inside it.
            None if !seen => {
                let output = self.space.outputs().next()?.clone();
                let area = self.space.output_geometry(&output)?.to_f64();
                at = Point::from((
                    at.x.clamp(area.loc.x, area.loc.x + area.size.w - 1.0),
                    at.y.clamp(area.loc.y, area.loc.y + area.size.h - 1.0),
                ));
                output
            }
            None => return None,
        };
        let lang = self.shell_lang();
        let said = seat.pending.as_ref().map(|p| p.intent.clone()).or_else(|| {
            seat.said
                .clone()
                .filter(|_| now.saturating_sub(seat.last) < SAID_FOR)
        });
        let label = match said {
            Some(said) => format!("{} · {said}", seat.name),
            None => seat.name.clone(),
        };
        let around = seat.pending.as_ref().and_then(|p| p.around);
        let (ring, confirm) = match seat.pending.as_ref().map(|p| (&p.phase, p)) {
            Some((Phase::Showing { .. }, _)) => (true, None),
            Some((Phase::Binding { .. } | Phase::Settling { .. }, p)) => {
                (matches!(p.action, Action::Click { .. }), None)
            }
            Some((Phase::Confirming { why, keys, .. }, p)) => {
                let why = match why {
                    Sensitive::Password => lang.sensitive_password().to_owned(),
                    Sensitive::App(app) => lang.sensitive_app(app),
                };
                // Where no screen shows the window, the card says where it
                // is.
                let why = match seat.on.filter(|_| !seen).and_then(|id| {
                    let desktop = self.model.desktop_of(id)?;
                    self.desktop_label(desktop)
                }) {
                    Some(desktop) => lang.sensitive_unseen(&why, &desktop),
                    None => why,
                };
                (
                    matches!(p.action, Action::Click { .. }),
                    Some(Confirm {
                        title: lang
                            .agent_will(&seat.name, !matches!(p.action, Action::Click { .. })),
                        intent: p.intent.clone(),
                        why,
                        keys: *keys,
                    }),
                )
            }
            None => (false, None),
        };
        Some((
            output,
            at,
            Showing {
                pointer: seen,
                label,
                ring,
                around: around.filter(|_| ring),
                confirm,
            },
        ))
    }

    /// Makes again the views whose content changed, drops the ones that
    /// hide; draws them.
    fn redraw_seats(&mut self) {
        let now = self.clock.now();
        let generation = self.config_generation;
        let mut wanted = Vec::with_capacity(self.seats.list.len());
        for seat in &self.seats.list {
            wanted.push(
                self.seat_showing(seat, now)
                    .and_then(|(output, at, showing)| {
                        let area = self.space.output_geometry(&output)?;
                        let local = at - area.loc.to_f64();
                        let key = ViewKey {
                            output: output.name(),
                            at: (local.x.round() as i32, local.y.round() as i32),
                            showing,
                            generation,
                        };
                        Some((key, area.size, output.current_scale().fractional_scale()))
                    }),
            );
        }
        let lang = self.shell_lang();
        // The theme only when a view is made again: not at every dispatch.
        let rebuild = self
            .seats
            .list
            .iter()
            .zip(&wanted)
            .any(|(seat, wanted)| match wanted {
                Some((key, _, _)) => seat.view.as_ref().is_none_or(|(k, _)| k != key),
                None => false,
            });
        let base = rebuild.then(|| self.theme());
        let mut changed = false;
        for (seat, wanted) in self.seats.list.iter_mut().zip(wanted) {
            match wanted {
                None => {
                    changed |= seat.view.take().is_some();
                }
                Some((key, _, _)) if seat.view.as_ref().is_some_and(|(k, _)| *k == key) => {}
                Some((key, size, scale)) => {
                    let Some(base) = &base else {
                        continue;
                    };
                    let theme = hocus_shell::seat::agent_theme(base, &seat.name);
                    let mut view = SeatView::new(
                        theme,
                        lang,
                        scale as f32,
                        hocus_ui::Size::new(size.w as f32, size.h as f32),
                        hocus_ui::Point::new(key.at.0 as f32, key.at.1 as f32),
                        &key.showing,
                    );
                    view.draw(now);
                    let mut panel = Panel::new(view);
                    panel.upload(&[]);
                    seat.view = Some((key, panel));
                    changed = true;
                }
            }
        }
        for seat in &mut self.seats.list {
            if let Some((_, panel)) = &mut seat.view {
                let drawn = panel.view.draw(now);
                changed |= panel.upload(&drawn);
            }
        }
        if changed {
            self.needs_redraw = true;
        }
    }

    /// The next moment a seat changes: the loop wakes then on real time.
    fn wake_for_seats(&mut self) {
        let now = self.clock.now();
        let next = self
            .seats
            .list
            .iter()
            .flat_map(|s| {
                let pending = s.pending.as_ref().map(|p| match p.phase {
                    Phase::Binding { until }
                    | Phase::Settling { until }
                    | Phase::Showing { until }
                    | Phase::Confirming { until, .. } => until,
                });
                [pending, Some(s.last + SAID_FOR), Some(s.last + SHOWN_FOR)]
            })
            .flatten()
            .chain(self.seats.retired.iter().map(|(_, at)| *at))
            .chain(self.seats.unseen_frame)
            .chain(self.seats.captures.iter().map(|w| w.until))
            .filter(|at| *at > now)
            .min();
        if next == self.seats.waking {
            return;
        }
        self.seats.waking = next;
        if let (Some(at), Some(handle)) = (next, self.waker()) {
            let timer = Timer::from_duration(at.saturating_sub(now));
            let woken = handle.insert_source(timer, |_, _, app| {
                app.timed("agent seats", |_| {});
                TimeoutAction::Drop
            });
            if let Err(err) = woken {
                tracing::warn!(%err.error, "an agent's pointer changes on the next event");
            }
        }
    }

    /// How many agents have a seat.
    pub fn seat_count(&self) -> usize {
        self.seats.list.len()
    }

    /// Actions agents wait on, and whether one waits for its window's
    /// application to take the seat.
    pub fn seat_pending(&self) -> (usize, bool) {
        let pending = self.seats.list.iter().filter_map(|s| s.pending.as_ref());
        let binding = pending
            .clone()
            .any(|p| matches!(p.phase, Phase::Binding { .. }));
        (pending.count(), binding)
    }

    /// Whether every window's application took every agent's seat, its
    /// pointer and its keyboard.
    pub fn seats_taken(&self) -> bool {
        use smithay::reexports::wayland_server::Resource as _;
        self.windows.values().all(|m| {
            let Some(client) = m.window.wl_surface().and_then(|s| s.client()) else {
                return true;
            };
            self.seats.list.iter().all(|s| {
                s.seat
                    .get_keyboard()
                    .is_some_and(|k| k.client_keyboards(&client).next().is_some())
                    && s.seat
                        .get_pointer()
                        .is_some_and(|p| p.client_pointers(&client).next().is_some())
            })
        })
    }

    /// The earliest moment an agent's action lands on its own (shown before
    /// a click), for the test harness to move its clock to; and whether one
    /// waits for the user instead.
    pub fn seat_deadline(&self) -> (Option<Duration>, bool) {
        let mut due = None;
        let mut asking = false;
        for p in self.seats.list.iter().filter_map(|s| s.pending.as_ref()) {
            match p.phase {
                Phase::Showing { until } | Phase::Settling { until } => {
                    due = Some(due.map_or(until, |d: Duration| d.min(until)));
                }
                Phase::Confirming { .. } => asking = true,
                Phase::Binding { .. } => {}
            }
        }
        (due, asking)
    }

    /// The seats' views on `output`, front first: the cards over the rest.
    pub fn seat_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<crate::scene::SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        if self.locked() {
            return Vec::new();
        }
        let name = output.name();
        let mut views: Vec<_> = self
            .seats
            .list
            .iter()
            .filter_map(|s| s.view.as_ref())
            .filter(|(k, _)| k.output == name)
            .collect();
        views.sort_by_key(|(k, _)| k.showing.confirm.is_none());
        let mut out = Vec::new();
        for (_, panel) in views {
            let o = panel.view.origin();
            let at = Point::from((o.x.round() as i32, o.y.round() as i32));
            let Some(card) = panel.element(renderer, output, at) else {
                continue;
            };
            out.push(crate::scene::SceneElement::Frame(card));
            // Its shadow under it, over the cards behind (#281).
            let shadows = crate::decoration::casters(at.to_f64(), &panel.view.shadows());
            out.extend(self.cast_elements(renderer, output, &shadows));
        }
        out
    }

    /// The seats' labels and cards, for the control API.
    pub fn seat_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        for (key, panel) in self.seats.list.iter().filter_map(|s| s.view.as_ref()) {
            let Some(area) = self
                .find_output(&key.output)
                .and_then(|o| self.space.output_geometry(&o))
            else {
                continue;
            };
            let o = panel.view.origin();
            let at = area.loc + Point::from((o.x.round() as i32, o.y.round() as i32));
            crate::shell::flatten(&panel.view.access(), at, 0, out);
        }
    }

    /// The user's pointer over an agent's card: it takes the pointer
    /// there. `pressed` for a button. A click on a card gives it the
    /// keyboard (#229); a press elsewhere takes it back.
    pub fn seat_card_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        if self.seats.injecting || self.locked() {
            return false;
        }
        let mut hit = None;
        for seat in &mut self.seats.list {
            let Some((key, panel)) = &mut seat.view else {
                continue;
            };
            let (Some(output), Some(card)) = (
                self.space.outputs().find(|o| o.name() == key.output),
                panel.view.card_rect(),
            ) else {
                continue;
            };
            let Some(area) = self.space.output_geometry(output) else {
                continue;
            };
            let o = panel.view.origin();
            let local = hocus_ui::Point::new(
                (at.x - f64::from(area.loc.x)) as f32 - o.x.round(),
                (at.y - f64::from(area.loc.y)) as f32 - o.y.round(),
            );
            panel.view.pointer_move(local);
            if hit.is_none() && card.contains(local) {
                let answer = pressed.and_then(|p| panel.view.pointer_button(p));
                hit = Some((seat.connection, answer));
            }
        }
        // Let go on a card (not on its buttons, which answer), it has the
        // keyboard; pressed elsewhere, none has it.
        let keys_to = match (pressed, hit) {
            (Some(false), Some((c, None))) => Some(Some(c)),
            (Some(true), None) => Some(None),
            _ => None,
        };
        if let Some(clicked) = keys_to {
            for seat in &mut self.seats.list {
                if let Some(Pending {
                    phase: Phase::Confirming { keys, .. },
                    ..
                }) = &mut seat.pending
                    && *keys != (Some(seat.connection) == clicked)
                {
                    *keys = !*keys;
                    self.needs_redraw = true;
                }
            }
        }
        let Some((connection, answer)) = hit else {
            return false;
        };
        if let Some(answer) = answer {
            self.seat_answer(connection, answer);
        }
        self.needs_redraw = true;
        true
    }

    /// Whether an agent's card has the user's keyboard: they clicked it,
    /// and it waits still.
    pub fn seat_card_keys(&self) -> bool {
        self.seats.list.iter().any(|s| {
            s.pending
                .as_ref()
                .is_some_and(|p| matches!(p.phase, Phase::Confirming { keys: true, .. }))
        })
    }

    /// A key for the card that has the keyboard: Enter presses the button
    /// with the keyboard (Allow first), Escape refuses, Tab moves; the
    /// rest does nothing.
    pub fn seat_card_key(&mut self, sym: smithay::input::keyboard::Keysym) {
        use hocus_ui::Key;
        use smithay::input::keyboard::Keysym;
        let key = match sym {
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::space => Key::Space,
            Keysym::Escape => Key::Escape,
            Keysym::Tab => Key::Tab,
            Keysym::ISO_Left_Tab => Key::BackTab,
            _ => return,
        };
        let Some(seat) = self.seats.list.iter_mut().find(|s| {
            s.pending
                .as_ref()
                .is_some_and(|p| matches!(p.phase, Phase::Confirming { keys: true, .. }))
        }) else {
            return;
        };
        let connection = seat.connection;
        // The view shows the card with the keyboard, made at the last
        // dispatch; else Enter allows, as Allow has the keyboard first.
        let answer = match seat
            .view
            .as_mut()
            .filter(|(k, _)| k.showing.confirm.as_ref().is_some_and(|c| c.keys))
        {
            Some((_, panel)) => panel.view.key(key),
            None => match key {
                Key::Enter | Key::Space => Some(Answer::Allow),
                Key::Escape => Some(Answer::Refuse),
                _ => None,
            },
        };
        if let Some(answer) = answer {
            self.seat_answer(connection, answer);
        }
        self.needs_redraw = true;
    }
}

impl State {
    /// Each agent's window drawn small in its slot of the column's journal,
    /// live: the column leaves the slot empty for it.
    pub(crate) fn seat_slot_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<crate::scene::SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let (Some(shell), Some(rect), Some(area)) = (
            self.shell.as_ref(),
            self.column_rect(output),
            self.space.output_geometry(output),
        ) else {
            return Vec::new();
        };
        let windows = self.seat_windows();
        let scale = output.current_scale().fractional_scale();
        let mut elements = Vec::new();
        for (agent, slot) in shell.column.seat_slots() {
            let Some((_, window)) = windows.iter().find(|(a, _)| *a == agent) else {
                continue;
            };
            let inset = 6.0;
            let slot = smithay::utils::Rectangle::<f64, Logical>::new(
                (
                    f64::from(rect.loc.x - area.loc.x) + f64::from(slot.x) + inset,
                    f64::from(rect.loc.y - area.loc.y) + f64::from(slot.y) + inset,
                )
                    .into(),
                (
                    (f64::from(slot.w) - 2.0 * inset).max(1.0),
                    (f64::from(slot.h) - 2.0 * inset).max(1.0),
                )
                    .into(),
            );
            elements.extend(crate::alt_tab::preview_elements(
                renderer, window, slot, scale, 1.0,
            ));
        }
        elements
    }
}
