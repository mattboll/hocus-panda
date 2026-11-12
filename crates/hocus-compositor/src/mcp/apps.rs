//! An MCP agent in applications' windows (docs/design/mcp.md, "In
//! applications"): the actions through its seat, the confirmation card
//! (scene 3) and taking over, the trees it aims by, `wait_for`, closing.
//!
//! Everything waits on timers of the loop: an application taking the seat
//! (polled every [`POLL`], [`super::seat::BIND_WAIT`] at most), a window
//! closing ([`CLOSE_WAIT`]), the card ([`hocus_link::MCP_CARD_WAIT`]), a
//! `wait_for`. Trees are read on [`super::atspi`]'s thread.
//!
//! Ported from v0-first-build's `seat.rs` (its phases: binding, settling
//! before typing, showing, confirming; take over and hand back) as loop
//! timers instead of v0's checks after each dispatch.

use std::time::{Duration, Instant};

use hocus_agents::mcp::{Call, WaitFor};
use hocus_link::{McpAsk, McpCard, McpDoing, McpKind, McpPointer, McpRefusal, McpReply, McpWhy};
use serde_json::{Value, json};
use smithay::{
    output::Output,
    reexports::{
        calloop::{
            RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::seat::WaylandFocus,
};

use super::{Waiting, atspi, seat::BIND_WAIT, seat::DRIVEN_FOR};
use crate::{State, model::WindowId};

/// How long typing waits once the agent's keyboard came into a window,
/// for its application to say what its field is for.
pub const SETTLE: Duration = Duration::from_millis(300);
/// How long a window asked to close is watched.
const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// How often what waits looks again.
const POLL: Duration = Duration::from_millis(100);
/// `wait_for`'s default.
const WAIT_FOR: u32 = 5000;
/// The room a card takes beside its point, at most.
const CARD_ROOM: (f64, f64) = (340.0, 330.0);
/// The pointer's place in a window that is not a click's: its top, its
/// middle, on the frame's header.
const TOP: f64 = 12.0;

/// What waits for a window's tree.
#[derive(Debug)]
pub enum TreeFor {
    /// `accessibility_tree`'s answer.
    Tool { session: u64, id: u64 },
    /// A click or a scroll aimed at an element.
    Aim(Box<Waiting>),
    /// A `wait_for` of an element.
    Wait(u64),
}

/// The action waiting for the user's answer.
#[derive(Debug)]
pub struct Card {
    pub shown: McpCard,
    waiting: Waiting,
    timer: Option<RegistrationToken>,
}

/// A `wait_for` under way.
#[derive(Debug)]
pub struct Wait {
    key: u64,
    session: u64,
    id: u64,
    ask: WaitFor,
    until: Instant,
    /// Its tree is being read.
    reading: bool,
}

/// A window asked to close, watched.
#[derive(Debug)]
pub struct Closing {
    waiting: Waiting,
    window: WindowId,
    surface: Option<WlSurface>,
    until: Instant,
}

/// The window a click or a scroll aims at by an element's label.
pub fn aims_at_label(call: &Call) -> Option<WindowId> {
    match call {
        Call::Click(c) if c.label.is_some() && c.x.is_none() && c.y.is_none() => {
            Some(WindowId(c.window))
        }
        Call::Scroll(c) if c.label.is_some() && c.x.is_none() && c.y.is_none() => {
            Some(WindowId(c.window))
        }
        _ => None,
    }
}

/// The window an action types into.
pub fn types_into(call: &Call) -> Option<WindowId> {
    match call {
        Call::Type(c) => Some(WindowId(c.window)),
        Call::PressKeys(c) => Some(WindowId(c.window)),
        _ => None,
    }
}

impl State {
    /// Runs `then` in `after`, on the loop; its token.
    fn mcp_timer(
        &mut self,
        after: Duration,
        then: impl FnOnce(&mut State) + 'static,
    ) -> Option<RegistrationToken> {
        let mut then = Some(then);
        self.loop_handle
            .insert_source(Timer::from_duration(after), move |_, _, app| {
                if let Some(then) = then.take() {
                    then(&mut app.state);
                }
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(error = %err.error, "an MCP timer"))
            .ok()
    }

    /// Connection `session` went: what it waited for goes.
    pub(super) fn mcp_session_gone(&mut self, session: u64) {
        if self
            .mcp
            .card
            .as_ref()
            .is_some_and(|c| c.waiting.session == session)
            && let Some(card) = self.mcp.card.take()
        {
            if let Some(timer) = card.timer {
                self.loop_handle.remove(timer);
            }
            self.mcp.busy = false;
        }
        if self
            .mcp
            .retry
            .as_ref()
            .is_some_and(|w| w.session == session)
        {
            self.mcp.retry = None;
            self.mcp.busy = false;
        }
        if self
            .mcp
            .closing
            .as_ref()
            .is_some_and(|c| c.waiting.session == session)
        {
            self.mcp.closing = None;
            self.mcp.busy = false;
        }
        self.mcp.waits.retain(|w| w.session != session);
        self.mcp.trees.retain(|_, t| match t {
            TreeFor::Tool { session: s, .. } => *s != session,
            TreeFor::Aim(w) => w.session != session,
            TreeFor::Wait(_) => true,
        });
        self.mcp.shot_waits.retain(|w| w.session != session);
        self.mcp_next();
    }

    /// What waits for the user or the application, taken away (every
    /// agent stopped): refused by the caller.
    pub(super) fn mcp_held(&mut self) -> Vec<Waiting> {
        let mut held = Vec::new();
        if let Some(card) = self.mcp.card.take() {
            if let Some(timer) = card.timer {
                self.loop_handle.remove(timer);
            }
            held.push(card.waiting);
        }
        if let Some(retry) = self.mcp.retry.take() {
            held.push(retry);
        }
        self.mcp.busy = self.mcp.closing.is_some();
        held
    }

    /// Where the agent's pointer goes for an action on window `id`
    /// (global): a click's or a scroll's point; for typing, where its
    /// pointer is in the window; else the window's top, its middle.
    pub(super) fn mcp_aim_point(
        &self,
        waiting: &Waiting,
        id: WindowId,
    ) -> Option<Point<f64, Logical>> {
        match &waiting.call {
            Call::Click(c) => self.mcp_point(id, c.x, c.y).ok(),
            Call::Scroll(c) => self.mcp_point(id, c.x, c.y).ok(),
            call => {
                let kept = types_into(call).and_then(|_| {
                    self.mcp_seat(waiting.session)
                        .and_then(|s| s.at)
                        .filter(|(_, on)| *on == id)
                        .map(|(at, _)| at)
                });
                kept.or_else(|| {
                    let rect = self.home_rect(id)?;
                    Some(Point::from((
                        f64::from(rect.x) + f64::from(rect.width) / 2.0,
                        f64::from(rect.y) + TOP.min(f64::from(rect.height) / 2.0),
                    )))
                })
            }
        }
    }

    /// The agent's pointer goes to `point` of window `id` as its action
    /// shows: there for a click or a scroll (the application sees it
    /// come), drawn there for the rest.
    pub(super) fn mcp_pointer_shown(
        &mut self,
        waiting: &Waiting,
        id: WindowId,
        point: Point<f64, Logical>,
    ) {
        if matches!(waiting.call, Call::Click(_) | Call::Scroll(_)) {
            self.mcp_pointer_to(waiting.session, id, point);
        } else if let Some(seat) = self
            .mcp
            .seats
            .list
            .iter_mut()
            .find(|s| s.session == Some(waiting.session))
        {
            seat.at = Some((point, id));
        }
        if let Some(seat) = self
            .mcp
            .seats
            .list
            .iter_mut()
            .find(|s| s.session == Some(waiting.session))
        {
            seat.last = Some(Instant::now());
        }
        self.mcp_pointer_timer();
    }

    /// The pointers hide [`DRIVEN_FOR`] after their agent last acted.
    fn mcp_pointer_timer(&mut self) {
        if let Some(old) = self.mcp.pointer_timer.take() {
            self.loop_handle.remove(old);
        }
        self.mcp.pointer_timer = self.mcp_timer(DRIVEN_FOR + POLL, |state| {
            state.mcp.pointer_timer = None;
            state.mcp.dirty = true;
            state.redraw = true;
        });
        self.mcp.dirty = true;
        self.redraw = true;
    }

    /// Each agent's pointer that shows now (global), and whether it is
    /// ringed: the one of the action shown, those that acted in the last
    /// [`DRIVEN_FOR`], the one whose card waits; where a screen shows the
    /// window it points at.
    fn mcp_pointers(&self) -> Vec<(String, u64, Point<f64, Logical>, bool)> {
        let showing = self
            .mcp
            .showing
            .as_ref()
            .filter(|s| s.point.is_some())
            .map(|s| s.waiting.session);
        let asking = self.mcp.card.as_ref().map(|c| c.waiting.session);
        self.mcp
            .seats
            .list
            .iter()
            .filter_map(|s| {
                let (at, on) = s.at?;
                let session = s.session?;
                let recent = s.last.is_some_and(|l| l.elapsed() < DRIVEN_FOR);
                let now = showing == Some(session) || asking == Some(session);
                (self.model.is_shown(on) && self.windows.contains_key(&on) && (recent || now))
                    .then(|| (s.agent.clone(), session, at, now))
            })
            .collect()
    }

    /// The window whose action waits on the card: ringed as it was shown.
    pub(super) fn mcp_card_window(&self) -> Option<WindowId> {
        let card = self.mcp.card.as_ref()?;
        card.waiting
            .call
            .window()
            .map(WindowId)
            .filter(|id| self.model.is_shown(*id))
    }

    /// The pointers on `output`, for its drawing.
    pub(crate) fn mcp_pointers_on(&self, output: &Output) -> Vec<(Point<f64, Logical>, bool)> {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let area = area.to_f64();
        self.mcp_pointers()
            .into_iter()
            .filter(|(_, _, at, _)| area.contains(*at))
            .map(|(_, _, at, ring)| (at, ring))
            .collect()
    }

    /// The resting pointers the shell names (not the one of the action
    /// shown, whose tag says its intent).
    pub(super) fn mcp_pointers_told(&self) -> Vec<McpPointer> {
        self.mcp_pointers()
            .into_iter()
            .filter(|(.., ring)| !ring)
            .filter_map(|(agent, _, at, _)| {
                let output = self.space.output_under(at).next()?;
                let origin = self.space.output_geometry(output)?.loc;
                Some(McpPointer {
                    agent,
                    output: output.name(),
                    x: (at.x - f64::from(origin.x)).round() as i32,
                    y: (at.y - f64::from(origin.y)).round() as i32,
                })
            })
            .collect()
    }

    /// An action shown long enough: done, or on its card first when the
    /// user must allow it.
    pub(super) fn mcp_go(&mut self, waiting: Waiting) {
        match self.mcp_sensitive(&waiting) {
            Some((why, ask)) => self.mcp_card(waiting, why, ask),
            None => self.mcp_do(waiting),
        }
    }

    /// Why the user must allow it first, and what it asks; none if they
    /// need not. Scrolling never asks.
    fn mcp_sensitive(&self, waiting: &Waiting) -> Option<(McpWhy, McpAsk)> {
        let id = WindowId(waiting.call.window()?);
        let title = crate::windows::title_of(self, id);
        let ask = match &waiting.call {
            Call::Click(_) => McpAsk::Click { title },
            Call::Type(_) => McpAsk::Type { title },
            Call::PressKeys(c) => McpAsk::Keys {
                title,
                keys: c.keys.clone(),
            },
            Call::CloseWindow(_) => McpAsk::Close { title },
            _ => return None,
        };
        let app = self
            .windows
            .get(&id)
            .and_then(|m| crate::windows::app_id(&m.window))
            .unwrap_or_default();
        if self.config.agents.confirms(&app) {
            return Some((McpWhy::App { name: app }, ask));
        }
        if matches!(waiting.call, Call::Type(_) | Call::PressKeys(_))
            && self.mcp_secret_field(waiting.session, id)
        {
            return Some((McpWhy::Password, ask));
        }
        None
    }

    /// Its card shows, beside the point, until the user answers or
    /// [`hocus_link::MCP_CARD_WAIT`] passes.
    fn mcp_card(&mut self, waiting: Waiting, why: McpWhy, ask: McpAsk) {
        self.mcp.dirty = true;
        self.mcp.next_card += 1;
        let card = self.mcp.next_card;
        let id = waiting.call.window().map(WindowId);
        let point = id.and_then(|id| self.mcp_aim_point(&waiting, id));
        let place = point.and_then(|at| {
            let output = id
                .and_then(|id| self.output_of(id))
                .or_else(|| self.space.outputs().next().cloned())?;
            let origin = self.space.output_geometry(&output)?;
            // Where no screen shows it: the card at the top of its screen.
            let local = if id.is_some_and(|id| self.model.is_shown(id)) {
                at - origin.loc.to_f64()
            } else {
                Point::from((f64::from(origin.size.w) / 2.0, 40.0))
            };
            // Inside its screen, the card's 300 px beside the point.
            let (w, h) = (f64::from(origin.size.w), f64::from(origin.size.h));
            let local = Point::<f64, Logical>::from((
                local.x.min(w - CARD_ROOM.0).max(0.0),
                local.y.min(h - CARD_ROOM.1).max(0.0),
            ));
            Some((output.name(), local))
        });
        let (output, local) = place.unwrap_or_default();
        tracing::info!(
            agent = waiting.agent,
            ?why,
            "an MCP agent's action waits for the user"
        );
        self.mcp_doing(&waiting.agent, McpDoing::Asking);
        let shown = McpCard {
            id: card,
            agent: waiting.agent.clone(),
            intent: waiting.intent.clone(),
            ask,
            why,
            output,
            x: local.x.round() as i32,
            y: local.y.round() as i32,
        };
        let timer = self.mcp_timer(hocus_link::MCP_CARD_WAIT, move |state| {
            state.mcp_card_answered(card, None);
        });
        self.mcp.busy = true;
        self.mcp.card = Some(Card {
            shown,
            waiting,
            timer,
        });
        self.redraw = true;
    }

    /// The user answered card `card` (the shell's buttons, its keys).
    pub(crate) fn mcp_reply(&mut self, card: u64, reply: McpReply) {
        self.mcp_card_answered(card, Some(reply));
    }

    /// Card `card` answered, or not in time (`None`).
    fn mcp_card_answered(&mut self, card: u64, reply: Option<McpReply>) {
        if self.mcp.card.as_ref().is_none_or(|c| c.shown.id != card) {
            return;
        }
        let Some(card) = self.mcp.card.take() else {
            return;
        };
        if let Some(timer) = card.timer
            && reply.is_some()
        {
            self.loop_handle.remove(timer);
        }
        self.mcp.busy = false;
        self.mcp.dirty = true;
        self.redraw = true;
        let mut waiting = card.waiting;
        let agent = waiting.agent.clone();
        tracing::info!(agent, ?reply, "the user answered an MCP agent");
        self.mcp_doing(&agent, McpDoing::Acting(super::kind_of(&waiting.call)));
        let refused = match reply {
            Some(McpReply::Allow) => {
                waiting.confirmed = true;
                // Checked again: the rights, the lock, the window.
                if let Some(why) = self.mcp_refusal(&agent, &waiting.call) {
                    Some(why)
                } else {
                    return self.mcp_do(waiting);
                }
            }
            Some(McpReply::Refuse) => Some(McpRefusal::UserRefused),
            None => Some(McpRefusal::NoAnswer),
            Some(McpReply::TakeOver) => {
                self.mcp_take_over(&agent, waiting.call.window().map(WindowId));
                Some(McpRefusal::TakenOver)
            }
        };
        if let Some(why) = refused {
            self.mcp_refuse(waiting.session, waiting.id, &agent, &waiting.call, why);
        }
        if !self.mcp.taken.contains(&agent) {
            self.mcp_after(&agent);
            if let Some(seen) = self.mcp.seen.iter_mut().find(|s| s.name == agent)
                && seen.doing == McpDoing::Asking
            {
                seen.doing = McpDoing::Ready;
            }
        }
        self.mcp_next();
    }

    /// « Je prends la main »: the window comes forward with the user's
    /// keyboard, the agent's keyboard lets go of it, and its actions are
    /// refused until « Rendre la main ».
    fn mcp_take_over(&mut self, agent: &str, window: Option<WindowId>) {
        self.mcp.taken.insert(agent.to_owned());
        self.mcp_doing(agent, McpDoing::Taken);
        let seats: Vec<_> = self
            .mcp
            .seats
            .list
            .iter()
            .filter(|s| s.agent == agent)
            .map(|s| s.seat.clone())
            .collect();
        for seat in seats {
            if let Some(keyboard) = seat.get_keyboard() {
                keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
            }
        }
        if let Some(id) = window.filter(|id| self.windows.contains_key(id)) {
            self.bring(id);
        }
        tracing::info!(agent, "the user took over from an MCP agent");
        self.mcp.dirty = true;
    }

    /// « Rendre la main »: agent `agent`'s actions go on.
    pub(crate) fn mcp_hand_back(&mut self, agent: &str) {
        if self.mcp.taken.remove(agent) {
            tracing::info!(agent, "the user handed back to an MCP agent");
            self.mcp_doing(agent, McpDoing::Ready);
            self.mcp.dirty = true;
        }
    }

    /// A click, a scroll, typing, keys or closing, done through the agent's
    /// own seat; answered later when its application has not taken the
    /// seat yet, or when the window is asked to close.
    pub(super) fn mcp_do_in_app(&mut self, waiting: Waiting) {
        let Some(id) = waiting.call.window().map(WindowId) else {
            return self.mcp_done(&waiting, Err("no window".into()));
        };
        if let Call::CloseWindow(_) = waiting.call {
            return self.mcp_close(waiting, id);
        }
        let keys = types_into(&waiting.call).is_some();
        if !self.mcp_seat_taken(waiting.session, id, keys) {
            if self.mcp_seat(waiting.session).is_none() {
                return self.mcp_done(&waiting, Err("this agent has no seat".into()));
            }
            if waiting.since.elapsed() < BIND_WAIT {
                return self.mcp_later(waiting);
            }
            return self.mcp_done(
                &waiting,
                Err(
                    "the window's application did not take the agent's seat: it handles the \
                     first seat alone, and only the user's input reaches it"
                        .into(),
                ),
            );
        }
        let agent = waiting.agent.clone();
        self.mcp_request(&agent, &waiting.intent, McpKind::Input);
        let said = self.mcp_window_said(id);
        let session = waiting.session;
        let result = match &waiting.call {
            Call::Click(c) => self.mcp_point(id, c.x, c.y).map(|at| {
                self.mcp_pointer_to(session, id, at);
                let button = c.button.unwrap_or_default();
                self.mcp_click(session, id, button.code(), c.count.unwrap_or(1));
                self.mcp_counted(&agent, |s| s.clicked += 1);
                let rect = self.home_rect(id).unwrap_or_default();
                let (x, y) = (at.x - f64::from(rect.x), at.y - f64::from(rect.y));
                (
                    json!({ "window": id.0, "x": x, "y": y }),
                    format!("click at {x},{y} of {said}"),
                )
            }),
            Call::Scroll(c) => self.mcp_point(id, c.x, c.y).map(|at| {
                self.mcp_pointer_to(session, id, at);
                self.mcp_scroll(session, c.dy);
                self.mcp_counted(&agent, |s| s.scrolled += 1);
                (
                    json!({ "window": id.0, "dy": c.dy }),
                    format!("scroll {} on {said}", c.dy),
                )
            }),
            Call::Type(c) => self.mcp_text_chords(&c.text).map(|chords| {
                self.mcp_keys_to(session, id);
                self.mcp_press(session, &chords);
                let n = c.text.chars().count() as u32;
                self.mcp_counted(&agent, |s| s.typed += n);
                // What is typed is never kept: its length alone.
                (
                    json!({ "typed": n }),
                    format!("type {n} characters in {said}"),
                )
            }),
            Call::PressKeys(c) => self.mcp_chord(&c.keys).map(|chord| {
                self.mcp_keys_to(session, id);
                self.mcp_press(session, &[chord]);
                self.mcp_counted(&agent, |s| s.pressed += 1);
                (
                    json!({ "keys": c.keys }),
                    format!("keys {} in {said}", c.keys),
                )
            }),
            _ => Err("not an action in an application".into()),
        };
        if result.is_ok() {
            self.mcp_acted(session, id);
            self.mcp_pointer_timer();
        }
        self.mcp_done(&waiting, result);
    }

    /// Done again in a moment: the application may take the seat meanwhile.
    fn mcp_later(&mut self, waiting: Waiting) {
        self.mcp.busy = true;
        self.mcp.retry = Some(waiting);
        self.mcp_timer(POLL, |state| {
            if let Some(waiting) = state.mcp.retry.take() {
                state.mcp.busy = false;
                state.mcp_do_in_app(waiting);
            }
        });
    }

    /// Window `id` asked to close, then watched: gone, a dialog of its own,
    /// or still there.
    fn mcp_close(&mut self, waiting: Waiting, id: WindowId) {
        let agent = waiting.agent.clone();
        self.mcp_keep_window(&agent, id, &waiting.intent);
        // Its place is kept for nothing: a closed window does not come back.
        if let Some(request) = self.mcp.open.get_mut(&agent) {
            request.before.windows.remove(&id);
        }
        let surface = self
            .windows
            .get(&id)
            .and_then(|m| m.window.wl_surface().map(|s| s.into_owned()));
        self.close_window(id);
        self.mcp_acted(waiting.session, id);
        self.mcp.busy = true;
        self.mcp.closing = Some(Closing {
            waiting,
            window: id,
            surface,
            until: Instant::now() + CLOSE_WAIT,
        });
        self.mcp_timer(POLL, State::mcp_close_poll);
    }

    fn mcp_close_poll(&mut self) {
        let Some(closing) = self.mcp.closing.as_ref() else {
            return;
        };
        let id = closing.window;
        let gone = !self.windows.contains_key(&id);
        // A window of its own whose parent it is: a "save changes?".
        let dialog = closing.surface.as_ref().and_then(|surface| {
            self.windows.iter().find_map(|(other, m)| {
                let parent = m.window.toplevel()?.parent()?;
                (parent == *surface).then(|| crate::windows::title_of(self, *other))
            })
        });
        let late = Instant::now() >= closing.until;
        if !gone && dialog.is_none() && !late {
            self.mcp_timer(POLL, State::mcp_close_poll);
            return;
        }
        let Some(closing) = self.mcp.closing.take() else {
            return;
        };
        self.mcp.busy = false;
        let agent = closing.waiting.agent.clone();
        let said = format!("window {}", id.0);
        let result = if gone {
            self.mcp_counted(&agent, |s| s.closed += 1);
            Ok((json!({ "closed": true }), format!("close {said}")))
        } else if let Some(title) = dialog {
            Ok((
                json!({
                    "closed": false,
                    "dialog": title,
                    "why": "its application shows a dialog of its own, which may ask to save: \
                            act in it, or leave it to the user",
                }),
                format!("close {said}; it asks first"),
            ))
        } else {
            Ok((
                json!({
                    "closed": false,
                    "why": format!(
                        "its application did not close it within {} s",
                        CLOSE_WAIT.as_secs()
                    ),
                }),
                format!("close {said}; it stays"),
            ))
        };
        self.mcp_done(&closing.waiting, result);
    }

    /// Window `id`'s tree asked of the reader for `for_`.
    pub(super) fn mcp_tree(&mut self, for_: TreeFor, id: WindowId) -> Result<(), String> {
        let managed = self
            .windows
            .get(&id)
            .ok_or_else(|| format!("no window {}", id.0))?;
        let client = managed.window.wl_surface().and_then(|s| s.client());
        let pid = client
            .as_ref()
            .and_then(|c| c.get_credentials(&self.display_handle).ok())
            .map(|c| c.pid as u32);
        let alone = client.as_ref().is_some_and(|client| {
            self.windows
                .values()
                .filter(|m| m.window.wl_surface().and_then(|s| s.client()).as_ref() == Some(client))
                .count()
                == 1
        });
        let rect = self.home_rect(id).unwrap_or_default();
        let window = atspi::Window {
            pid,
            app_id: crate::windows::app_id(&managed.window),
            title: Some(crate::windows::title_of(self, id)),
            size: (rect.width, rect.height),
            alone,
        };
        if self.mcp.reader.is_none() {
            let bus = match (&self.config.a11y_bus, &self.shell.bus) {
                (Some(address), _) => atspi::Bus::Address(address.clone()),
                (None, Some(crate::shell::Bus::Given(address))) => {
                    atspi::Bus::Given(address.clone())
                }
                (None, Some(crate::shell::Bus::Session)) => atspi::Bus::Session,
                (None, None) => atspi::Bus::Unknown,
            };
            let sender = self.agents.sender.clone();
            self.mcp.reader = atspi::Reader::start(bus, move |token, outcome| {
                if let Some(sender) = &sender {
                    let _ = sender.send(crate::agents::Heard::Mcp(super::FromAgent::Tree {
                        token,
                        outcome,
                    }));
                }
            });
        }
        self.mcp.next_token += 1;
        let token = self.mcp.next_token;
        let asked = self
            .mcp
            .reader
            .as_ref()
            .is_some_and(|r| r.read(token, window));
        if !asked {
            self.mcp.reader = None;
            return Err("applications' trees cannot be read now".into());
        }
        self.mcp.trees.insert(token, for_);
        Ok(())
    }

    /// A tree read comes back to what asked it.
    pub(super) fn mcp_tree_read(&mut self, token: u64, outcome: atspi::Outcome) {
        let Some(for_) = self.mcp.trees.remove(&token) else {
            return;
        };
        match for_ {
            TreeFor::Tool { session, id } => {
                let result = match outcome {
                    Ok(tree) => Ok(tree.json()),
                    // No tree is an answer, said: not a failure.
                    Err(missing) => Ok(json!({ "elements": [], "note": missing.to_string() })),
                };
                self.mcp_answer(session, id, result);
            }
            TreeFor::Aim(mut waiting) => {
                let (role, label) = match &waiting.call {
                    Call::Click(c) => (c.role.clone(), c.label.clone()),
                    Call::Scroll(c) => (c.role.clone(), c.label.clone()),
                    _ => (None, None),
                };
                let at = outcome.map_err(|m| m.to_string()).and_then(|tree| {
                    atspi::centre_of(&tree, role.as_deref(), &label.unwrap_or_default())
                });
                let (x, y) = match at {
                    Ok(at) => at,
                    Err(why) => return self.mcp_answer(waiting.session, waiting.id, Err(why)),
                };
                match &mut waiting.call {
                    Call::Click(c) => (c.x, c.y) = (Some(x), Some(y)),
                    Call::Scroll(c) => (c.x, c.y) = (Some(x), Some(y)),
                    _ => {}
                }
                if let Err(why) = self.mcp_check(&waiting.call) {
                    return self.mcp_answer(waiting.session, waiting.id, Err(why));
                }
                self.mcp.queue.push_back(*waiting);
                self.mcp_next();
            }
            TreeFor::Wait(key) => {
                let found = outcome.ok().map(|tree| {
                    self.mcp
                        .waits
                        .iter()
                        .find(|w| w.key == key)
                        .and_then(|w| w.ask.label.clone())
                        .is_some_and(|label| {
                            !matches!(atspi::pick(&tree.nodes, None, &label), atspi::Pick::Nothing)
                        })
                });
                if let Some(wait) = self.mcp.waits.iter_mut().find(|w| w.key == key) {
                    wait.reading = false;
                }
                if let Some(found) = found {
                    self.mcp_wait_seen(key, found);
                }
            }
        }
    }

    /// `wait_for`: checked now, then every [`POLL`] until it comes (or
    /// goes) or its time is up.
    pub(super) fn mcp_wait(&mut self, session: u64, id: u64, ask: WaitFor) -> Result<(), String> {
        let by_label = ask.label.is_some();
        if by_label && ask.window.is_none() {
            return Err("wait_for: an element is looked for in a window: give window".into());
        }
        if !by_label && ask.app_id.is_none() && ask.title.is_none() {
            return Err(
                "wait_for: say what to wait for: app_id, title, or window and label".into(),
            );
        }
        if let Some(window) = ask.window {
            self.mcp_window(window)?;
        }
        let ms = ask
            .timeout_ms
            .unwrap_or(WAIT_FOR)
            .min(hocus_agents::mcp::MAX_WAIT_MS);
        self.mcp.next_token += 1;
        let key = self.mcp.next_token;
        self.mcp.waits.push(Wait {
            key,
            session,
            id,
            ask,
            until: Instant::now() + Duration::from_millis(u64::from(ms)),
            reading: false,
        });
        self.mcp_wait_poll();
        Ok(())
    }

    fn mcp_wait_poll(&mut self) {
        self.mcp.wait_timer = None;
        let keys: Vec<u64> = self.mcp.waits.iter().map(|w| w.key).collect();
        for key in keys {
            let Some(wait) = self.mcp.waits.iter().find(|w| w.key == key) else {
                continue;
            };
            if Instant::now() >= wait.until {
                self.mcp_wait_end(key, false);
                continue;
            }
            match (wait.ask.window, wait.ask.label.is_some(), wait.reading) {
                (Some(window), true, false) => {
                    let window = WindowId(window);
                    if !self.windows.contains_key(&window) {
                        let gone = wait.ask.gone;
                        self.mcp_wait_end(key, gone);
                        continue;
                    }
                    if self.mcp_tree(TreeFor::Wait(key), window).is_ok()
                        && let Some(wait) = self.mcp.waits.iter_mut().find(|w| w.key == key)
                    {
                        wait.reading = true;
                    }
                }
                (_, true, true) | (None, true, false) => {}
                (_, false, _) => {
                    let (app, title) = (wait.ask.app_id.clone(), wait.ask.title.clone());
                    let there = self.windows.iter().any(|(id, m)| {
                        app.as_deref()
                            .is_none_or(|a| crate::windows::app_id(&m.window).as_deref() == Some(a))
                            && title.as_deref().is_none_or(|t| {
                                crate::windows::title_of(self, *id)
                                    .to_lowercase()
                                    .contains(&t.to_lowercase())
                            })
                    });
                    self.mcp_wait_seen(key, there);
                }
            }
        }
        if !self.mcp.waits.is_empty() && self.mcp.wait_timer.is_none() {
            self.mcp.wait_timer = self.mcp_timer(POLL, State::mcp_wait_poll);
        }
    }

    /// What wait `key` looks for is there (or not): it ends if that is
    /// what it waits for.
    fn mcp_wait_seen(&mut self, key: u64, there: bool) {
        let Some(wait) = self.mcp.waits.iter().find(|w| w.key == key) else {
            return;
        };
        if there != wait.ask.gone {
            self.mcp_wait_end(key, true);
        }
    }

    fn mcp_wait_end(&mut self, key: u64, came: bool) {
        let Some(i) = self.mcp.waits.iter().position(|w| w.key == key) else {
            return;
        };
        let wait = self.mcp.waits.remove(i);
        let word = if wait.ask.gone { "gone" } else { "shown" };
        let mut value = json!({ "done": came });
        if let Value::Object(fields) = &mut value {
            fields.insert(
                "said".into(),
                json!(if came {
                    format!("it is {word}")
                } else {
                    format!("not {word} in time")
                }),
            );
        }
        self.mcp_answer(wait.session, wait.id, Ok(value));
    }
}
