//! What an MCP agent does by hand (#825, docs/design/mcp.md, "Dragging
//! and drawing"): `drag` presses, moves and releases its own pointer in a
//! window, in steps on a timer of the loop; `draw` draws a shape in its
//! ink ([`crate::ink::agent`]) over a short time, its pointer following its
//! hand. Both keep the queue waiting until they are done; the tool answers
//! then.
//!
//! Without « Dessiner à l'écran » a `draw` is refused before anything
//! shows: its row says so, and one toast offers « Permettre… ».

use std::time::{Duration, Instant};

use hocus_agents::mcp::{Call, Draw, MAX_POINTS, MAX_WORD, Shape};
use hocus_link::{MCP_TOAST, McpRefusal, McpRefusedToast};
use serde_json::json;
use smithay::{
    backend::input::ButtonState,
    input::pointer::ButtonEvent,
    output::Output,
    reexports::calloop::RegistrationToken,
    utils::{Logical, Point, SERIAL_COUNTER},
};

use super::Waiting;
use crate::{
    State,
    ink::agent::{Plan, around},
    model::WindowId,
};

/// A drag moves its pointer this often.
const DRAG_STEP: Duration = Duration::from_millis(16);
/// It moves at this pace, in this time at least and at most.
const DRAG_PACE: f64 = 900.0;
const DRAG_LEAST: Duration = Duration::from_millis(350);
const DRAG_MOST: Duration = Duration::from_millis(1000);

/// A shape being drawn.
#[derive(Debug)]
pub struct Drawing {
    waiting: Waiting,
    /// Its mark in the agents' ink.
    key: (u64, usize),
    output: String,
    shape: Shape,
}

/// A drag under way: its button held, its pointer between its ends.
#[derive(Debug)]
pub struct Dragging {
    waiting: Waiting,
    window: WindowId,
    from: Point<f64, Logical>,
    to: Point<f64, Logical>,
    step: u32,
    steps: u32,
    button: u32,
    timer: Option<RegistrationToken>,
}

fn shape_word(shape: Shape) -> &'static str {
    match shape {
        Shape::Rectangle => "rectangle",
        Shape::Ellipse => "ellipse",
        Shape::Arrow => "arrow",
        Shape::Path => "path",
    }
}

/// The hand's ease, in and out (the mockup's `ease.io`).
fn ease_io(x: f64) -> f64 {
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}

impl State {
    /// Whether a `draw` can be drawn as asked.
    pub(super) fn mcp_draw_check(&self, c: &Draw) -> Result<(), String> {
        if let Some(word) = &c.label
            && word.trim().chars().count() > MAX_WORD
        {
            return Err(format!(
                "draw: label is one short word beside the shape, {MAX_WORD} characters at most"
            ));
        }
        match (&c.points, c.window) {
            (Some(points), _) => {
                let wanted = match c.shape {
                    Shape::Path => 2..=MAX_POINTS,
                    _ => 2..=2,
                };
                if !wanted.contains(&points.len()) {
                    return Err(match c.shape {
                        Shape::Path => format!("draw: a path takes 2 to {MAX_POINTS} points"),
                        Shape::Arrow => {
                            "draw: an arrow takes 2 points, its tail then its tip".into()
                        }
                        _ => "draw: a rectangle or an ellipse takes 2 points, opposite corners"
                            .into(),
                    });
                }
                let first = points.first().map(|p| Point::from((p[0], p[1])));
                if first.is_none_or(|p| self.space.output_under(p).next().is_none()) {
                    return Err("draw: its first point is on no screen (list_outputs)".into());
                }
                Ok(())
            }
            (None, Some(window)) => {
                let id = self.mcp_window(window)?;
                if !self.model.is_shown(id) {
                    return Err(format!(
                        "draw: window {window} is on no screen now: nothing to draw around"
                    ));
                }
                Ok(())
            }
            (None, None) => Err(
                "draw: say where: points (global logical pixels), or window (and element)".into(),
            ),
        }
    }

    /// What `waiting`'s `draw` draws, and on which screen.
    fn mcp_draw_plan(&self, waiting: &Waiting) -> Result<(Output, Plan), String> {
        let Call::Draw(c) = &waiting.call else {
            return Err("not a drawing".into());
        };
        let seed = (waiting.id as u32).wrapping_mul(2_654_435_761) ^ 0x825;
        let (points, hand_made): (Vec<(f64, f64)>, bool) = match (&c.points, c.window) {
            (Some(points), _) => (points.iter().map(|p| (p[0], p[1])).collect(), false),
            (None, Some(window)) => {
                let rect = match c.around {
                    Some([x, y, w, h]) => (x, y, w, h),
                    None => {
                        let r = self
                            .home_rect(WindowId(window))
                            .ok_or("draw: the window went")?;
                        (
                            f64::from(r.x),
                            f64::from(r.y),
                            f64::from(r.width),
                            f64::from(r.height),
                        )
                    }
                };
                let centre = Point::from((rect.0 + rect.2 / 2.0, rect.1 + rect.3 / 2.0));
                let room = self
                    .space
                    .output_under(centre)
                    .next()
                    .and_then(|o| self.space.output_geometry(o))
                    .ok_or("draw: the window is on no screen")?;
                let room = (
                    f64::from(room.loc.x),
                    f64::from(room.loc.y),
                    f64::from(room.size.w),
                    f64::from(room.size.h),
                );
                around(c.shape, rect, room, seed)
            }
            (None, None) => return Err("draw: say where".into()),
        };
        let first = points.first().copied().unwrap_or_default();
        let output = self
            .space
            .output_under(Point::from(first))
            .next()
            .or_else(|| {
                let (x, y) = points.last().copied().unwrap_or_default();
                self.space.output_under(Point::from((x, y))).next()
            })
            .cloned()
            .ok_or("draw: the shape is on no screen")?;
        let area = self
            .space
            .output_geometry(&output)
            .ok_or("draw: the screen went")?;
        let origin = (f64::from(area.loc.x), f64::from(area.loc.y));
        let plan = Plan {
            shape: c.shape,
            points: points
                .iter()
                .map(|p| (p.0 - origin.0, p.1 - origin.1))
                .collect(),
            hand_made,
            word: c
                .label
                .as_deref()
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(str::to_owned),
            room: (f64::from(area.size.w), f64::from(area.size.h)),
            seed,
        };
        Ok((output, plan))
    }

    /// Where a `draw`'s hand starts (global), on which screen.
    pub(super) fn mcp_draw_start(
        &self,
        waiting: &Waiting,
    ) -> Option<(Output, Point<f64, Logical>)> {
        let (output, plan) = self.mcp_draw_plan(waiting).ok()?;
        let origin = self.space.output_geometry(&output)?.loc.to_f64();
        let start = crate::ink::agent::lines(&plan)
            .first()
            .and_then(|l| l.first().copied())?;
        Some((output, origin + Point::from(start)))
    }

    /// The agent's pointer on the screen at `point` (global), on no window.
    pub(super) fn mcp_pointer_on_screen(&mut self, session: u64, point: Point<f64, Logical>) {
        if let Some(seat) = self.mcp_seat_mut(session) {
            seat.ink_at = Some(point);
            seat.last = Some(Instant::now());
        }
        self.mcp_pointer_timer();
    }

    /// A `draw` shown long enough: drawn by hand, the tool answered once
    /// it is whole.
    pub(super) fn mcp_draw_go(&mut self, waiting: Waiting) {
        let (output, plan) = match self.mcp_draw_plan(&waiting) {
            Ok(planned) => planned,
            Err(why) => return self.mcp_done(&waiting, Err(why)),
        };
        let agent = waiting.agent.clone();
        self.mcp_request(&agent, &waiting.intent, hocus_link::McpKind::Draw);
        let now = self.now();
        let name = output.name();
        let crate::ink::Ink {
            agent: ink,
            agent_ids,
            ..
        } = &mut self.ink;
        let Some((key, total)) = ink.draw(&agent, name.clone(), &plan, now, agent_ids) else {
            return self.mcp_done(&waiting, Err("draw: nothing to draw there".into()));
        };
        tracing::debug!(
            agent,
            shape = shape_word(plan.shape),
            ?total,
            "an MCP agent draws"
        );
        self.mcp.busy = true;
        self.mcp.dirty = true;
        self.redraw = true;
        let shape = plan.shape;
        self.mcp.drawing = Some(Drawing {
            waiting,
            key,
            output: name,
            shape,
        });
        if self.mcp_timer(total, State::mcp_drawn).is_none() {
            self.mcp_drawn();
        }
    }

    /// The shape being drawn is whole: the tool answers.
    fn mcp_drawn(&mut self) {
        let Some(drawing) = self.mcp.drawing.take() else {
            return;
        };
        self.mcp.busy = false;
        self.mcp.dirty = true;
        self.redraw = true;
        let agent = drawing.waiting.agent.clone();
        let result = if self.ink.agent.finish(drawing.key) {
            // Its pointer rests where its hand ended.
            let now = self.now();
            let tip = self.ink.agent.mark(drawing.key).and_then(|m| m.tip(now));
            let origin = self
                .space
                .outputs()
                .find(|o| o.name() == drawing.output)
                .and_then(|o| self.space.output_geometry(o))
                .map(|g| g.loc.to_f64());
            if let (Some(tip), Some(origin)) = (tip, origin) {
                self.mcp_pointer_on_screen(drawing.waiting.session, origin + Point::from(tip));
            }
            self.mcp_counted(&agent, |s| s.drawn += 1);
            let word = shape_word(drawing.shape);
            Ok((
                json!({ "drawn": word, "output": drawing.output }),
                format!("{word} on {}", drawing.output),
            ))
        } else {
            Err(
                "draw: its shape went before it was whole (the session locked, or its screen \
                 went)"
                    .into(),
            )
        };
        self.mcp_done(&drawing.waiting, result);
    }

    /// The window a drag is under way in: ringed until it lets go, as the
    /// mockup's.
    pub(super) fn mcp_drag_window(&self) -> Option<WindowId> {
        self.mcp
            .dragging
            .as_ref()
            .map(|d| d.window)
            .filter(|id| self.model.is_shown(*id))
    }

    /// Whether a shape is being drawn.
    pub(super) fn mcp_drawing(&self) -> bool {
        self.mcp.drawing.is_some()
    }

    /// The connection whose pointer moves now, drawing or dragging.
    pub(super) fn mcp_moving_session(&self) -> Option<u64> {
        self.mcp
            .drawing
            .as_ref()
            .map(|d| d.waiting.session)
            .or_else(|| self.mcp.dragging.as_ref().map(|d| d.waiting.session))
    }

    /// The agent drawing has its pointer at its hand's tip, as of `now`
    /// (each frame).
    pub(crate) fn mcp_draw_follow(&mut self, now: Duration) {
        let Some(drawing) = self.mcp.drawing.as_ref() else {
            return;
        };
        let session = drawing.waiting.session;
        let tip = self.ink.agent.mark(drawing.key).and_then(|m| m.tip(now));
        let origin = self
            .space
            .outputs()
            .find(|o| o.name() == drawing.output)
            .and_then(|o| self.space.output_geometry(o))
            .map(|g| g.loc.to_f64());
        if let (Some(tip), Some(origin)) = (tip, origin)
            && let Some(seat) = self.mcp_seat_mut(session)
        {
            seat.ink_at = Some(origin + Point::from(tip));
            seat.last = Some(Instant::now());
        }
    }

    /// Connection `session` went: its shape is whole, its drag let go.
    pub(super) fn mcp_hand_gone(&mut self, session: u64) {
        if self
            .mcp
            .drawing
            .as_ref()
            .is_some_and(|d| d.waiting.session == session)
            && let Some(drawing) = self.mcp.drawing.take()
        {
            self.ink.agent.finish(drawing.key);
            self.mcp.busy = false;
        }
        if self
            .mcp
            .dragging
            .as_ref()
            .is_some_and(|d| d.waiting.session == session)
        {
            self.mcp_drag_let_go();
            self.mcp.busy = false;
        }
    }

    /// The drawing toast's « Effacer »: `agent`'s shapes fade.
    pub(crate) fn mcp_erase(&mut self, agent: &str) {
        let now = self.now();
        if self.ink.agent.fade(Some(agent), now) {
            tracing::info!(agent, "an MCP agent's drawing erased");
            self.mcp.dirty = true;
            self.redraw = true;
        }
    }

    /// One toast for a call refused for want of a right: kept as it is
    /// while it shows for the same agent and tool.
    pub(super) fn mcp_refused_toast(&mut self, agent: &str, tool: &str, why: McpRefusal) {
        if self
            .mcp
            .refused
            .as_ref()
            .is_some_and(|(t, _)| t.agent == agent && t.tool == tool)
        {
            return;
        }
        let Ok(output) = self.mcp_output(None) else {
            return;
        };
        let (Some(area), Some(geometry)) =
            (self.work_area(&output), self.space.output_geometry(&output))
        else {
            return;
        };
        if let Some((_, timer)) = self.mcp.refused.take() {
            self.loop_handle.remove(timer);
        }
        if let Some((_, timer)) = self.mcp.toast.take() {
            self.loop_handle.remove(timer);
        }
        self.mcp.next_refused += 1;
        let id = self.mcp.next_refused;
        let toast = McpRefusedToast {
            id,
            agent: agent.to_owned(),
            tool: tool.to_owned(),
            why,
            output: output.name(),
            left: area.x - geometry.loc.x + super::TOAST_IN,
            bottom: area.y - geometry.loc.y + area.height - super::TOAST_IN,
        };
        let timer = self.mcp_timer(MCP_TOAST, move |state| {
            if state.mcp.refused.as_ref().is_some_and(|(t, _)| t.id == id) {
                state.mcp.refused = None;
                state.mcp.dirty = true;
            }
        });
        if let Some(timer) = timer {
            self.mcp.refused = Some((toast, timer));
        }
        self.mcp.dirty = true;
    }

    // --- Dragging --------------------------------------------------------------------

    /// A drag's press at its start, then its steps on a timer.
    pub(super) fn mcp_drag_start(&mut self, waiting: Waiting, id: WindowId) {
        let Call::Drag(c) = &waiting.call else {
            return self.mcp_done(&waiting, Err("not a drag".into()));
        };
        let (from, to) = match (
            self.mcp_point(id, c.x, c.y),
            self.mcp_point(id, c.to_x, c.to_y),
        ) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(why), _) | (_, Err(why)) => return self.mcp_done(&waiting, Err(why)),
        };
        let button = c.button.unwrap_or_default().code();
        let session = waiting.session;
        self.mcp_keys_to(session, id);
        self.mcp_pointer_to(session, id, from);
        self.mcp_button(session, button, ButtonState::Pressed);
        if let Some(seat) = self.mcp_seat_mut(session) {
            seat.pressed = true;
        }
        let distance = (to.x - from.x).hypot(to.y - from.y);
        let time = Duration::from_secs_f64(distance / DRAG_PACE).clamp(DRAG_LEAST, DRAG_MOST);
        let steps = (time.as_millis() / DRAG_STEP.as_millis()).max(2) as u32;
        tracing::debug!(agent = waiting.agent, steps, "an MCP agent drags");
        self.mcp.busy = true;
        self.mcp.dragging = Some(Dragging {
            waiting,
            window: id,
            from,
            to,
            step: 0,
            steps,
            button,
            timer: None,
        });
        self.mcp_drag_next();
    }

    fn mcp_drag_next(&mut self) {
        let timer = self.mcp_timer(DRAG_STEP, State::mcp_drag_step);
        if let Some(dragging) = self.mcp.dragging.as_mut() {
            dragging.timer = timer;
        }
    }

    /// One step of the drag: its pointer further along, released at its
    /// end.
    fn mcp_drag_step(&mut self) {
        let Some(dragging) = self.mcp.dragging.as_mut() else {
            return;
        };
        dragging.timer = None;
        dragging.step += 1;
        let u = ease_io(f64::from(dragging.step) / f64::from(dragging.steps));
        let (from, to) = (dragging.from, dragging.to);
        let at = Point::from((from.x + (to.x - from.x) * u, from.y + (to.y - from.y) * u));
        let (session, window, done) = (
            dragging.waiting.session,
            dragging.window,
            dragging.step >= dragging.steps,
        );
        if self.windows.contains_key(&window) {
            self.mcp_pointer_to(session, window, at);
            self.mcp_acted(session, window);
        }
        self.redraw = true;
        if !done {
            return self.mcp_drag_next();
        }
        let Some(dragging) = self.mcp.dragging.take() else {
            return;
        };
        self.mcp_button(session, dragging.button, ButtonState::Released);
        if let Some(seat) = self.mcp_seat_mut(session) {
            seat.pressed = false;
        }
        self.mcp.busy = false;
        self.mcp_pointer_timer();
        let agent = dragging.waiting.agent.clone();
        let result = if self.windows.contains_key(&window) {
            let said = self.mcp_window_said(window);
            let rect = self.home_rect(window).unwrap_or_default();
            let local = |p: Point<f64, Logical>| json!({ "x": p.x - f64::from(rect.x), "y": p.y - f64::from(rect.y) });
            self.mcp_counted(&agent, |s| s.dragged += 1);
            Ok((
                json!({ "window": window.0, "from": local(from), "to": local(to) }),
                format!(
                    "drag from {},{} to {},{} of {said}",
                    from.x - f64::from(rect.x),
                    from.y - f64::from(rect.y),
                    to.x - f64::from(rect.x),
                    to.y - f64::from(rect.y)
                ),
            ))
        } else {
            Err("drag: the window went during the drag".into())
        };
        self.mcp_done(&dragging.waiting, result);
    }

    /// A drag under way stops: its button released where its pointer is;
    /// what it waited for, to be refused.
    pub(super) fn mcp_drag_let_go(&mut self) -> Option<Waiting> {
        let dragging = self.mcp.dragging.take()?;
        if let Some(timer) = dragging.timer {
            self.loop_handle.remove(timer);
        }
        let session = dragging.waiting.session;
        self.mcp_button(session, dragging.button, ButtonState::Released);
        if let Some(seat) = self.mcp_seat_mut(session) {
            seat.pressed = false;
        }
        Some(dragging.waiting)
    }

    /// The button `button` of `session`'s pointer, pressed or released.
    fn mcp_button(&mut self, session: u64, button: u32, state: ButtonState) {
        let Some(pointer) = self.mcp_seat(session).and_then(|s| s.seat.get_pointer()) else {
            return;
        };
        let time = self.mcp_time();
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
    }
}
