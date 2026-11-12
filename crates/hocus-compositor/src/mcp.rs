//! The agents that act on the desktop through `hocus mcp`
//! (docs/design/mcp.md), kept on the loop.
//!
//! Each `hocus mcp --agent NAME` holds a line on the control socket
//! ([`hocus_agents::mcp`]): a thread of its own reads it ([`serve`]) and
//! hands each call to the loop, which answers it on that line through the
//! thread that writes it. The loop holds the agents to their rights
//! (`[[agents.mcp]]`, read at every call), answers what they read at once,
//! and shows what they will do before doing it: one action at a time, its
//! intent shown for `[agents] show-before-ms` (a timer of the loop, never
//! a sleep), the rights checked again, then done. Settings are written on
//! a thread of their own (hocus-config), the answer sent once it is back.
//!
//! An agent's actions make a request until 1.5 s pass without a new one:
//! then a toast sums it up, and the request keeps what it changed as it
//! was before (windows' places, desktops shown, settings), for "Annuler".
//! The journal is in memory only, the last [`JOURNAL`] calls acted on or
//! refused; it reaches the log at debug level alone. `Super+Shift+Escape`
//! (or the journal's "Tout arrêter") stops every agent until let go.

use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use hocus_agents::mcp::{Answer, Asked, Call};
use hocus_config::toml_edit;
use hocus_link::{
    MCP_JOURNAL_SHOWN, MCP_TOAST, McpAt, McpDoing, McpKind, McpLine, McpRefusal, McpRow, McpSaid,
    McpShow, McpShown, McpSummary, McpToast, ToShell,
};
use serde_json::{Value, json};
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture, element::Id},
    output::Output,
    reexports::calloop::{
        RegistrationToken, channel,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Rectangle},
};

use crate::{
    State,
    agents::Heard,
    config::McpAccess,
    decoration::{self, BoxStyle, box_elements},
    model::{Mode, Rect, WindowId, Zone},
    render::{OutputElement, OutputElements},
};

/// The calls the journal keeps, the last ones.
pub const JOURNAL: usize = 500;
/// A request ends this long after its last action.
pub const SETTLE: Duration = Duration::from_millis(1500);
/// The requests kept for "Annuler".
const REQUESTS: usize = 20;
/// Sections an agent never writes: its own rights, the keys (the stop
/// key), what only a developer turns on.
const FORBIDDEN: [&str; 3] = ["agents", "shortcuts", "developer"];
/// The window's ring: 2 px, 2 px outside it (the mockup's `outline: 2px
/// solid var(--agent-ring); outline-offset: 2px`).
const RING_WIDTH: f64 = 2.0;
const RING_OFFSET: f64 = 2.0;
/// `--agent-ring`: the agent's violet (the theme's `agent`) at 55 %.
const RING_ALPHA: f32 = 0.55;
/// The tag's place inside a window's top left corner.
const TAG_IN: i32 = 12;
/// The tag's top on a screen, under its work area's top.
const TAG_TOP: i32 = 16;
/// The toast's distance from the work area's bottom left corner.
const TOAST_IN: i32 = 16;

static SESSIONS: AtomicU64 = AtomicU64::new(1);

/// What an agent's line hands the loop.
#[derive(Debug)]
pub enum FromAgent {
    /// An agent's line opened: where its answers go.
    Opened {
        session: u64,
        agent: String,
        to: mpsc::Sender<String>,
    },
    Line {
        session: u64,
        line: String,
    },
    Closed {
        session: u64,
    },
    /// The settings read, for `get_settings`.
    Read {
        session: u64,
        id: u64,
        result: Result<Value, String>,
    },
    /// A setting written: what it was before, or why it was not.
    Written {
        waiting: Box<Waiting>,
        key: String,
        result: Result<Option<toml_edit::Value>, String>,
    },
    /// A request's settings written back.
    Restored {
        failed: Vec<String>,
    },
}

/// Serves an agent's line, from the control socket's thread: one thread
/// reads the calls and hands them to the loop, another writes the answers.
pub fn serve(stream: UnixStream, loop_: channel::Sender<Heard>, agent: String) {
    let session = SESSIONS.fetch_add(1, Ordering::Relaxed);
    let (to, answers) = mpsc::channel::<String>();
    let _ = stream.set_read_timeout(None);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let started = std::thread::Builder::new()
        .name("mcp line out".to_owned())
        .spawn(move || {
            let _ = writer.set_write_timeout(Some(Duration::from_secs(5)));
            for line in answers {
                if writer.write_all(line.as_bytes()).is_err() || writer.write_all(b"\n").is_err() {
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
        });
    if started.is_err() {
        return;
    }
    // Its hello: `hocus mcp` sends no call before it, so that none is
    // read by the control socket's thread, which read the first line.
    let _ = to.send(Answer::ok(0, json!("hocuspanda")).line());
    let heard = |from| loop_.send(Heard::Mcp(from)).is_ok();
    if !heard(FromAgent::Opened { session, agent, to }) {
        return;
    }
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        if !heard(FromAgent::Line { session, line }) {
            return;
        }
    }
    let _ = loop_.send(Heard::Mcp(FromAgent::Closed { session }));
}

/// An agent's line.
#[derive(Debug)]
struct Conn {
    agent: String,
    to: mpsc::Sender<String>,
}

/// An action waiting to be shown and done.
#[derive(Debug)]
pub struct Waiting {
    session: u64,
    id: u64,
    agent: String,
    call: Call,
    intent: String,
}

/// The action shown now, before it is done.
#[derive(Debug)]
struct Showing {
    waiting: Waiting,
    show: McpShow,
    window: Option<WindowId>,
    timer: Option<RegistrationToken>,
}

/// Where a window was.
#[derive(Debug, Clone, Copy)]
struct Placed {
    rect: Rect,
    desktop: usize,
    mode: Mode,
    minimized: bool,
}

/// What a request changed, as it was before.
#[derive(Debug, Default)]
struct Before {
    windows: BTreeMap<WindowId, Placed>,
    /// The desktop each screen showed.
    desktops: BTreeMap<String, usize>,
    /// Each setting's value (none: the file did not have it).
    settings: BTreeMap<String, Option<toml_edit::Value>>,
}

impl Before {
    fn undoable(&self) -> bool {
        !self.windows.is_empty() || !self.desktops.is_empty() || !self.settings.is_empty()
    }
}

/// An agent's request: its actions until it settles, then kept for
/// "Annuler".
#[derive(Debug)]
struct Request {
    id: u64,
    agent: String,
    intent: String,
    summary: McpSummary,
    before: Before,
    /// The desktop it worked on last (from 1).
    desktop: Option<u32>,
    kind: McpKind,
    settle: Option<RegistrationToken>,
    undone: bool,
}

/// A call as the journal keeps it.
#[derive(Debug, Clone)]
struct Entry {
    at: String,
    agent: String,
    tool: &'static str,
    what: String,
    intent: Option<String>,
    refused: Option<String>,
}

/// An agent seen this session, for its row.
#[derive(Debug)]
struct Seen {
    id: u64,
    name: String,
    doing: McpDoing,
}

/// The agents, their lines, what they show and what they did.
#[derive(Debug, Default)]
pub struct Mcp {
    conns: BTreeMap<u64, Conn>,
    seen: Vec<Seen>,
    journal: VecDeque<Entry>,
    lines: VecDeque<McpLine>,
    pub stopped: bool,
    queue: VecDeque<Waiting>,
    showing: Option<Showing>,
    /// A setting being written: the next action waits for it.
    busy: bool,
    /// Requests under way, by agent.
    open: BTreeMap<String, Request>,
    /// Settled requests, the newest last.
    done: VecDeque<Request>,
    next_request: u64,
    toast: Option<(McpToast, RegistrationToken)>,
    /// What the shell was told last.
    pub told: Option<McpShown>,
    ring_ids: RingIds,
}

#[derive(Debug)]
struct RingIds([Id; 9]);

impl Default for RingIds {
    fn default() -> Self {
        Self(std::array::from_fn(|_| Id::new()))
    }
}

/// The clock's time, as the user reads it.
fn clock(seconds: bool) -> String {
    let now = jiff::Zoned::now();
    if seconds {
        now.strftime("%H:%M:%S").to_string()
    } else {
        now.strftime("%H:%M").to_string()
    }
}

/// What a refusal tells the agent.
fn refusal_text(refusal: &McpRefusal, agent: &str) -> String {
    match refusal {
        McpRefusal::Unknown => format!(
            "refused: no agent {agent:?} in the configuration's [[agents.mcp]]; the user names \
             the agents that may use the desktop there"
        ),
        McpRefusal::ReadOnly => format!(
            "refused: agent {agent:?} may only read the desktop ([[agents.mcp]] access = \"read\")"
        ),
        McpRefusal::NoSettings => {
            format!("refused: agent {agent:?} may not change settings ([[agents.mcp]] settings)")
        }
        McpRefusal::Locked => "refused: the session is locked".to_owned(),
        McpRefusal::Asking => "refused: a password is being asked for".to_owned(),
        McpRefusal::Stopped => {
            "refused: the user stopped every agent; they may let them go on".to_owned()
        }
        McpRefusal::Forbidden => format!("refused: no agent changes [{}]", FORBIDDEN.join("], [")),
    }
}

/// A window's state as `list_windows` says it.
fn mode_word(mode: Mode) -> &'static str {
    match mode {
        Mode::Floating => "floating",
        Mode::Snapped(Zone::Maximized) => "maximized",
        Mode::Snapped(_) => "snapped",
        Mode::Fullscreen => "fullscreen",
    }
}

fn rect_json(r: Rect) -> Value {
    json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height })
}

impl State {
    pub(crate) fn mcp_heard(&mut self, from: FromAgent) {
        match from {
            FromAgent::Opened { session, agent, to } => {
                tracing::info!(agent, session, "an MCP agent connects");
                self.mcp_seen(&agent);
                self.mcp.conns.insert(session, Conn { agent, to });
            }
            FromAgent::Closed { session } => {
                if let Some(conn) = self.mcp.conns.remove(&session) {
                    tracing::info!(agent = conn.agent, session, "an MCP agent leaves");
                }
                self.mcp.queue.retain(|w| w.session != session);
                if self
                    .mcp
                    .showing
                    .as_ref()
                    .is_some_and(|s| s.waiting.session == session)
                {
                    self.mcp_unshow();
                    self.mcp_next();
                }
            }
            FromAgent::Line { session, line } => self.mcp_line(session, &line),
            FromAgent::Read {
                session,
                id,
                result,
            } => self.mcp_answer(session, id, result),
            FromAgent::Written {
                waiting,
                key,
                result,
            } => self.mcp_written(*waiting, key, result),
            FromAgent::Restored { failed } => {
                if !failed.is_empty() {
                    tracing::warn!(?failed, "settings not put back");
                }
            }
        }
    }

    /// An agent's row, made the first time it is seen.
    fn mcp_seen(&mut self, agent: &str) {
        if self.mcp.seen.iter().any(|s| s.name == agent) {
            return;
        }
        let id = self.mcp.seen.len() as u64 + 1;
        self.mcp.seen.push(Seen {
            id,
            name: agent.to_owned(),
            doing: McpDoing::Ready,
        });
    }

    fn mcp_doing(&mut self, agent: &str, doing: McpDoing) {
        if let Some(seen) = self.mcp.seen.iter_mut().find(|s| s.name == agent) {
            seen.doing = doing;
        }
    }

    fn mcp_answer(&mut self, session: u64, id: u64, result: Result<Value, String>) {
        let Some(conn) = self.mcp.conns.get(&session) else {
            return;
        };
        let answer = match result {
            Ok(value) => Answer::ok(id, value),
            Err(why) => Answer::error(id, why),
        };
        let _ = conn.to.send(answer.line());
    }

    fn mcp_journal(&mut self, entry: Entry) {
        tracing::debug!(
            agent = entry.agent,
            tool = entry.tool,
            what = entry.what,
            refused = entry.refused,
            "an MCP agent's call"
        );
        self.mcp.journal.push_back(entry);
        while self.mcp.journal.len() > JOURNAL {
            self.mcp.journal.pop_front();
        }
    }

    fn mcp_line_shown(&mut self, agent: &str, said: McpSaid) {
        self.mcp.lines.push_front(McpLine {
            agent: agent.to_owned(),
            at: clock(false),
            said,
        });
        self.mcp.lines.truncate(MCP_JOURNAL_SHOWN);
    }

    /// Why `agent` may not make `call` now, if it may not.
    fn mcp_refusal(&self, agent: &str, call: &Call) -> Option<McpRefusal> {
        let Some(rights) = self.config.agents.mcp.iter().find(|a| a.name == agent) else {
            return Some(McpRefusal::Unknown);
        };
        if self.mcp.stopped {
            return Some(McpRefusal::Stopped);
        }
        if self.locked() {
            return Some(McpRefusal::Locked);
        }
        if !call.acts() {
            return None;
        }
        if rights.access == McpAccess::Read {
            return Some(McpRefusal::ReadOnly);
        }
        if self.auth_holds() {
            return Some(McpRefusal::Asking);
        }
        if let Call::SetSetting(set) = call {
            if FORBIDDEN.contains(&set.key.split('.').next().unwrap_or_default()) {
                return Some(McpRefusal::Forbidden);
            }
            if !rights.settings {
                return Some(McpRefusal::NoSettings);
            }
        }
        None
    }

    /// Refuses call `id`, keeping it in the journal.
    fn mcp_refuse(&mut self, session: u64, id: u64, agent: &str, call: &Call, why: McpRefusal) {
        let text = refusal_text(&why, agent);
        self.mcp_journal(Entry {
            at: clock(true),
            agent: agent.to_owned(),
            tool: call.tool(),
            what: String::new(),
            intent: call.intent().map(str::to_owned),
            refused: Some(text.clone()),
        });
        if call.acts() || why == McpRefusal::Stopped {
            self.mcp_line_shown(
                agent,
                McpSaid::Refused {
                    tool: call.tool().to_owned(),
                    why,
                },
            );
        }
        self.mcp_answer(session, id, Err(text));
    }

    fn mcp_line(&mut self, session: u64, line: &str) {
        let Some(agent) = self.mcp.conns.get(&session).map(|c| c.agent.clone()) else {
            return;
        };
        let asked: Asked = match serde_json::from_str(line) {
            Ok(asked) => asked,
            Err(err) => {
                tracing::debug!(%err, "an MCP line that does not read");
                return;
            }
        };
        let call = match Call::of(&asked.tool, &asked.args) {
            Ok(call) => call,
            Err(why) => return self.mcp_answer(session, asked.id, Err(why)),
        };
        if let Some(why) = self.mcp_refusal(&agent, &call) {
            return self.mcp_refuse(session, asked.id, &agent, &call, why);
        }
        if !call.acts() {
            let result = self.mcp_read(session, asked.id, &agent, &call);
            if let Some(result) = result {
                self.mcp_answer(session, asked.id, result);
            }
            return;
        }
        let Some(intent) = call.intent().and_then(hocus_agents::mcp::intent_text) else {
            return self.mcp_answer(
                session,
                asked.id,
                Err(format!(
                    "{}: say what this does for the user, in one short line, with `intent`",
                    call.tool()
                )),
            );
        };
        if let Err(why) = self.mcp_check(&call) {
            return self.mcp_answer(session, asked.id, Err(why));
        }
        self.mcp.queue.push_back(Waiting {
            session,
            id: asked.id,
            agent,
            call,
            intent,
        });
        self.mcp_next();
    }

    /// A reading call's answer; none when a thread answers it later.
    fn mcp_read(
        &mut self,
        session: u64,
        id: u64,
        agent: &str,
        call: &Call,
    ) -> Option<Result<Value, String>> {
        Some(Ok(match call {
            Call::ListWindows(_) => json!({ "windows": self.mcp_windows() }),
            Call::ListDesktops(_) => json!({ "screens": self.mcp_desktops() }),
            Call::ListOutputs(_) => json!({ "outputs": self.mcp_outputs() }),
            Call::Journal(asked) => {
                let limit = asked.limit.unwrap_or(50).clamp(1, JOURNAL as u32) as usize;
                let mine: Vec<&Entry> = self
                    .mcp
                    .journal
                    .iter()
                    .filter(|e| e.agent == agent)
                    .collect();
                let from = mine.len().saturating_sub(limit);
                let entries: Vec<Value> = mine
                    .get(from..)
                    .unwrap_or_default()
                    .iter()
                    .map(|e| {
                        json!({
                            "at": e.at,
                            "tool": e.tool,
                            "what": e.what,
                            "intent": e.intent,
                            "refused": e.refused,
                        })
                    })
                    .collect();
                json!({ "entries": entries })
            }
            Call::GetSettings(asked) => {
                let Some(file) = self.config.file.clone() else {
                    return Some(Ok(
                        json!({ "settings": settings_now("", asked.section.as_deref()) }),
                    ));
                };
                let section = asked.section.clone();
                let sender = self.agents.sender.clone();
                crate::programs::on_a_thread("mcp settings".to_owned(), move || {
                    let result = hocus_config::read(&file).map_err(|e| e.to_string()).map(
                        |read| json!({ "settings": settings_now(&read.text, section.as_deref()) }),
                    );
                    if let Some(sender) = sender {
                        let _ = sender.send(Heard::Mcp(FromAgent::Read {
                            session,
                            id,
                            result,
                        }));
                    }
                });
                return None;
            }
            _ => return Some(Err("not a reading tool".to_owned())),
        }))
    }

    fn mcp_windows(&self) -> Vec<Value> {
        let focused = self.model.focused();
        self.model
            .stack()
            .iter()
            .filter_map(|id| {
                let managed = self.windows.get(id)?;
                let rect = self.home_rect(*id)?;
                Some(json!({
                    "id": id.0,
                    "title": crate::windows::title_of(self, *id),
                    "app_id": crate::windows::app_id(&managed.window),
                    "output": self.output_of(*id).map(|o| o.name()),
                    "desktop": self.model.desktop(*id) + 1,
                    "geometry": rect_json(rect),
                    "focused": focused == Some(*id),
                    "minimized": self.model.is_minimized(*id),
                    "state": mode_word(self.model.mode(*id)),
                }))
            })
            .collect()
    }

    fn mcp_desktops(&self) -> Vec<Value> {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        outputs
            .iter()
            .map(|output| {
                let strip = self.strip(output);
                let on = self.windows_on(output);
                let desktops: Vec<Value> = (0..strip.count())
                    .map(|d| {
                        json!({
                            "number": d + 1,
                            "name": self.config.desktops.name(d),
                            "windows": on.iter().filter(|id| self.model.desktop(**id) == d).count(),
                        })
                    })
                    .collect();
                json!({
                    "output": output.name(),
                    "current": strip.current() + 1,
                    "desktops": desktops,
                })
            })
            .collect()
    }

    fn mcp_outputs(&self) -> Vec<Value> {
        self.space
            .outputs()
            .map(|output| {
                json!({
                    "name": output.name(),
                    "geometry": self.space.output_geometry(output).map(|g| rect_json(crate::windows::rect_of(g))),
                    "work_area": self.work_area(output).map(rect_json),
                    "scale": output.current_scale().fractional_scale(),
                })
            })
            .collect()
    }

    /// Whether an action can be done as asked: the window there, the
    /// desktop on its screen, the setting a known one of the right type.
    fn mcp_check(&self, call: &Call) -> Result<(), String> {
        match call {
            Call::FocusWindow(c) => self.mcp_window(c.window).map(|_| ()),
            Call::MoveWindow(c) => {
                let id = self.mcp_window(c.window)?;
                if c.x.is_none()
                    && c.y.is_none()
                    && c.width.is_none()
                    && c.height.is_none()
                    && c.desktop.is_none()
                {
                    return Err("move_window: say where (x, y, width, height) or desktop".into());
                }
                if c.width.is_some_and(|w| w < 1) || c.height.is_some_and(|h| h < 1) {
                    return Err("move_window: a width and a height of 1 at least".into());
                }
                if let Some(desktop) = c.desktop {
                    let output = self
                        .output_of(id)
                        .ok_or("move_window: the window is on no screen")?;
                    self.mcp_desktop_on(&output, desktop)?;
                }
                Ok(())
            }
            Call::SwitchDesktop(c) => {
                let output = self.mcp_output(c.output.as_deref())?;
                self.mcp_desktop_on(&output, c.desktop).map(|_| ())
            }
            Call::Launch(c) => {
                if c.argv.first().is_none_or(String::is_empty) {
                    return Err(format!("launch: {} has nothing to run", c.name));
                }
                Ok(())
            }
            Call::SetSetting(c) => setting_value(&c.key, &c.value).map(|_| ()),
            _ => Ok(()),
        }
    }

    fn mcp_window(&self, id: u64) -> Result<WindowId, String> {
        let id = WindowId(id);
        if self.windows.contains_key(&id) {
            Ok(id)
        } else {
            Err(format!(
                "no window {}: list_windows says which there are",
                id.0
            ))
        }
    }

    /// The screen `name` names, else the focused window's, else the first.
    fn mcp_output(&self, name: Option<&str>) -> Result<Output, String> {
        match name {
            Some(name) => self
                .space
                .outputs()
                .find(|o| o.name() == name)
                .cloned()
                .ok_or_else(|| format!("no screen {name:?}: list_outputs says which there are")),
            None => self
                .model
                .focused()
                .and_then(|id| self.output_of(id))
                .or_else(|| self.space.outputs().next().cloned())
                .ok_or_else(|| "no screen".to_owned()),
        }
    }

    fn mcp_desktop_on(&self, output: &Output, desktop: u32) -> Result<usize, String> {
        let count = self.strip(output).count();
        match desktop as usize {
            d @ 1.. if d <= count => Ok(d - 1),
            _ => Err(format!(
                "no desktop {desktop} on {}: it has {count}, from 1",
                output.name()
            )),
        }
    }

    /// The next action shown, if none is.
    fn mcp_next(&mut self) {
        if self.mcp.showing.is_some() || self.mcp.busy {
            return;
        }
        let Some(waiting) = self.mcp.queue.pop_front() else {
            return;
        };
        if let Some(why) = self.mcp_refusal(&waiting.agent, &waiting.call) {
            self.mcp_refuse(
                waiting.session,
                waiting.id,
                &waiting.agent,
                &waiting.call,
                why,
            );
            return self.mcp_next();
        }
        if let Err(why) = self.mcp_check(&waiting.call) {
            self.mcp_answer(waiting.session, waiting.id, Err(why));
            return self.mcp_next();
        }
        let kind = kind_of(&waiting.call);
        self.mcp_doing(&waiting.agent, McpDoing::Acting(kind));
        let wait = Duration::from_millis(u64::from(self.config.agents.show_before_ms));
        let Some((show, window)) = self.mcp_show_of(&waiting) else {
            return self.mcp_do(waiting);
        };
        if wait.is_zero() {
            return self.mcp_do(waiting);
        }
        tracing::debug!(
            agent = waiting.agent,
            tool = waiting.call.tool(),
            "an MCP action shown"
        );
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(wait), |_, _, app| {
                app.state.mcp_shown_long_enough();
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(error = %err.error, "an MCP action is not shown"))
            .ok();
        self.mcp.showing = Some(Showing {
            waiting,
            show,
            window,
            timer,
        });
        self.redraw = true;
        if timer.is_none() {
            self.mcp_shown_long_enough();
        }
    }

    /// The action shown has shown long enough: done, if it still may be.
    fn mcp_shown_long_enough(&mut self) {
        let Some(showing) = self.mcp.showing.take() else {
            return;
        };
        self.redraw = true;
        let waiting = showing.waiting;
        if let Some(why) = self.mcp_refusal(&waiting.agent, &waiting.call) {
            self.mcp_refuse(
                waiting.session,
                waiting.id,
                &waiting.agent,
                &waiting.call,
                why,
            );
            self.mcp_after(&waiting.agent);
            return self.mcp_next();
        }
        self.mcp_do(waiting);
    }

    /// No action shown any more.
    fn mcp_unshow(&mut self) {
        if let Some(showing) = self.mcp.showing.take()
            && let Some(timer) = showing.timer
        {
            self.loop_handle.remove(timer);
        }
        self.redraw = true;
    }

    /// What an action shows, and the window it rings.
    fn mcp_show_of(&self, waiting: &Waiting) -> Option<(McpShow, Option<WindowId>)> {
        let show = |output: &Output, at: McpAt| McpShow {
            agent: waiting.agent.clone(),
            intent: waiting.intent.clone(),
            output: output.name(),
            at,
        };
        let top = |output: &Output| {
            let area = self.work_area(output)?;
            let origin = self.space.output_geometry(output)?.loc;
            Some((
                area.x - origin.x + area.width / 2,
                area.y - origin.y + TAG_TOP,
            ))
        };
        match &waiting.call {
            Call::FocusWindow(c) => self.mcp_window_show(WindowId(c.window), &show),
            Call::MoveWindow(c) => {
                let id = WindowId(c.window);
                match c.desktop {
                    // Sent away without a move on its desktop: its tile.
                    Some(desktop) if c.x.is_none() && c.y.is_none() && !self.model.is_shown(id) => {
                        let output = self.output_of(id)?;
                        let (x, y) = top(&output)?;
                        let index = u8::try_from(desktop.saturating_sub(1)).ok()?;
                        Some((show(&output, McpAt::Desktop { index, x, y }), None))
                    }
                    _ => self.mcp_window_show(id, &show),
                }
            }
            Call::SwitchDesktop(c) => {
                let output = self.mcp_output(c.output.as_deref()).ok()?;
                let (x, y) = top(&output)?;
                let index = u8::try_from(c.desktop.saturating_sub(1)).ok()?;
                Some((show(&output, McpAt::Desktop { index, x, y }), None))
            }
            Call::Launch(_) | Call::SetSetting(_) => {
                let output = self.mcp_output(None).ok()?;
                let (x, y) = top(&output)?;
                Some((show(&output, McpAt::Screen { x, y }), None))
            }
            _ => None,
        }
    }

    fn mcp_window_show(
        &self,
        id: WindowId,
        show: &dyn Fn(&Output, McpAt) -> McpShow,
    ) -> Option<(McpShow, Option<WindowId>)> {
        let output = self.output_of(id)?;
        let origin = self.space.output_geometry(&output)?.loc;
        if !self.model.is_shown(id) {
            // Not on screen: the tag at the top of its screen.
            let area = self.work_area(&output)?;
            let at = McpAt::Screen {
                x: area.x - origin.x + area.width / 2,
                y: area.y - origin.y + TAG_TOP,
            };
            return Some((show(&output, at), None));
        }
        let rect = self.home_rect(id)?;
        let at = McpAt::Window {
            x: rect.x - origin.x + TAG_IN,
            y: rect.y - origin.y + TAG_IN,
        };
        Some((show(&output, at), Some(id)))
    }

    /// Does an action, keeping what it changes for "Annuler".
    fn mcp_do(&mut self, waiting: Waiting) {
        let agent = waiting.agent.clone();
        let result = match &waiting.call {
            Call::FocusWindow(c) => {
                let id = WindowId(c.window);
                let what = self.mcp_window_said(id);
                self.mcp_keep_window(&agent, id, &waiting.intent);
                self.bring(id);
                self.mcp_counted(&agent, |s| s.focused += 1);
                Ok((json!({ "focused": id.0 }), what))
            }
            Call::MoveWindow(c) => self.mcp_move(&agent, &waiting.intent, c),
            Call::SwitchDesktop(c) => self.mcp_switch(&agent, &waiting.intent, c),
            Call::Launch(c) => {
                self.mcp_request(&agent, &waiting.intent, McpKind::Launch);
                let launch = hocus_link::Launch {
                    id: c.id.clone(),
                    terminal: c.terminal,
                    dir: c.dir.clone().unwrap_or_default(),
                    argv: c.argv.clone(),
                };
                self.launch(launch);
                let name = c.name.clone();
                self.mcp_counted(&agent, |s| s.launched.push(name));
                Ok((json!({ "launched": c.id }), c.name.clone()))
            }
            Call::SetSetting(_) => return self.mcp_write(waiting),
            _ => Err("not an acting tool".to_owned()),
        };
        match result {
            Ok((value, what)) => {
                self.mcp_journal(Entry {
                    at: clock(true),
                    agent: agent.clone(),
                    tool: waiting.call.tool(),
                    what,
                    intent: Some(waiting.intent.clone()),
                    refused: None,
                });
                self.mcp_answer(waiting.session, waiting.id, Ok(value));
            }
            Err(why) => self.mcp_answer(waiting.session, waiting.id, Err(why)),
        }
        self.mcp_after(&agent);
        self.mcp_next();
    }

    fn mcp_window_said(&self, id: WindowId) -> String {
        let app = self
            .windows
            .get(&id)
            .and_then(|m| crate::windows::app_id(&m.window))
            .unwrap_or_default();
        format!(
            "window {} {app} {:?}",
            id.0,
            crate::windows::title_of(self, id)
        )
    }

    fn mcp_move(
        &mut self,
        agent: &str,
        intent: &str,
        c: &hocus_agents::mcp::MoveWindow,
    ) -> Result<(Value, String), String> {
        let id = self.mcp_window(c.window)?;
        let what = self.mcp_window_said(id);
        self.mcp_keep_window(agent, id, intent);
        if c.x.is_some() || c.y.is_some() || c.width.is_some() || c.height.is_some() {
            if self.model.is_minimized(id) {
                self.unminimize(id);
            }
            let mut rect = self.home_rect(id).ok_or("the window went")?;
            if self.model.mode(id) != Mode::Floating {
                // Floating first, from where it floated.
                if let Some(back) = self.model.unsnap(id) {
                    rect = back;
                }
            }
            rect.x = c.x.unwrap_or(rect.x);
            rect.y = c.y.unwrap_or(rect.y);
            rect.width = c.width.unwrap_or(rect.width);
            rect.height = c.height.unwrap_or(rect.height);
            self.start_settle(id);
            self.configure(id, rect);
        }
        if let Some(desktop) = c.desktop {
            let output = self.output_of(id).ok_or("the window is on no screen")?;
            let to = self.mcp_desktop_on(&output, desktop)?;
            self.send_window_to_desktop(id, to, false);
            if let Some(request) = self.mcp.open.get_mut(agent) {
                request.desktop = Some(desktop);
            }
        } else if let Some(request) = self.mcp.open.get_mut(agent) {
            request.desktop = Some(self.model.desktop(id) as u32 + 1);
        }
        self.mcp_counted(agent, |s| s.moved += 1);
        let rect = self.target_rect(id).unwrap_or_default();
        Ok((
            json!({
                "window": id.0,
                "geometry": rect_json(rect),
                "desktop": self.model.desktop(id) + 1,
            }),
            what,
        ))
    }

    fn mcp_switch(
        &mut self,
        agent: &str,
        intent: &str,
        c: &hocus_agents::mcp::SwitchDesktop,
    ) -> Result<(Value, String), String> {
        let output = self.mcp_output(c.output.as_deref())?;
        let to = self.mcp_desktop_on(&output, c.desktop)?;
        self.mcp_request(agent, intent, McpKind::Desktop);
        let name = output.name();
        let current = self.strip(&output).current();
        if let Some(request) = self.mcp.open.get_mut(agent) {
            request
                .before
                .desktops
                .entry(name.clone())
                .or_insert(current);
            request.desktop = Some(c.desktop);
            request.summary.switched = Some(c.desktop);
        }
        self.switch_desktop(&output, to, None);
        Ok((
            json!({ "output": name, "desktop": c.desktop }),
            format!("desktop {} of {name}", c.desktop),
        ))
    }

    /// The agent's request under way, opened if none is; its settling
    /// timer started again.
    fn mcp_request(&mut self, agent: &str, intent: &str, kind: McpKind) {
        if !self.mcp.open.contains_key(agent) {
            self.mcp.next_request += 1;
            let request = Request {
                id: self.mcp.next_request,
                agent: agent.to_owned(),
                intent: intent.to_owned(),
                summary: McpSummary::default(),
                before: Before::default(),
                desktop: None,
                kind,
                settle: None,
                undone: false,
            };
            self.mcp.open.insert(agent.to_owned(), request);
        }
        let old = self.mcp.open.get_mut(agent).and_then(|r| {
            r.kind = kind;
            r.settle.take()
        });
        if let Some(old) = old {
            self.loop_handle.remove(old);
        }
        let name = agent.to_owned();
        let token = self
            .loop_handle
            .insert_source(Timer::from_duration(SETTLE), move |_, _, app| {
                app.state.mcp_settle(&name);
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(error = %err.error, "an MCP request does not settle"))
            .ok();
        if let Some(request) = self.mcp.open.get_mut(agent) {
            request.settle = token;
        }
    }

    fn mcp_counted(&mut self, agent: &str, count: impl FnOnce(&mut McpSummary)) {
        if let Some(request) = self.mcp.open.get_mut(agent) {
            count(&mut request.summary);
        }
    }

    /// Where window `id` was, kept the first time the request touches it.
    fn mcp_keep_window(&mut self, agent: &str, id: WindowId, intent: &str) {
        self.mcp_request(agent, intent, McpKind::Windows);
        let Some(rect) = self.home_rect(id) else {
            return;
        };
        let placed = Placed {
            rect: match self.model.mode(id) {
                Mode::Floating => rect,
                // Where it floats, to come back to it after a snap.
                _ => self.model.placement(id).restore.unwrap_or(rect),
            },
            desktop: self.model.desktop(id),
            mode: self.model.mode(id),
            minimized: self.model.is_minimized(id),
        };
        let shown = self
            .output_of(id)
            .map(|output| (output.name(), self.strip(&output).current()));
        if let Some(request) = self.mcp.open.get_mut(agent) {
            request.before.windows.entry(id).or_insert(placed);
            if let Some((name, current)) = shown {
                request.before.desktops.entry(name).or_insert(current);
            }
        }
    }

    /// A setting written, on a thread: the queue waits for it.
    fn mcp_write(&mut self, waiting: Waiting) {
        let Call::SetSetting(set) = &waiting.call else {
            return;
        };
        let Some(file) = self.config.file.clone() else {
            self.mcp_answer(
                waiting.session,
                waiting.id,
                Err("no configuration file to write: the desktop was started without one".into()),
            );
            self.mcp_after(&waiting.agent);
            return self.mcp_next();
        };
        let key = set.key.clone();
        let value = match setting_value(&set.key, &set.value) {
            Ok(value) => value,
            Err(why) => {
                self.mcp_answer(waiting.session, waiting.id, Err(why));
                self.mcp_after(&waiting.agent);
                return self.mcp_next();
            }
        };
        self.mcp_request(&waiting.agent, &waiting.intent, McpKind::Setting);
        self.mcp.busy = true;
        let sender = self.agents.sender.clone();
        crate::programs::on_a_thread("mcp setting".to_owned(), move || {
            let result = write_setting(&file, &key, Some(value));
            if let Some(sender) = sender {
                let _ = sender.send(Heard::Mcp(FromAgent::Written {
                    waiting: Box::new(waiting),
                    key,
                    result,
                }));
            }
        });
    }

    fn mcp_written(
        &mut self,
        waiting: Waiting,
        key: String,
        result: Result<Option<toml_edit::Value>, String>,
    ) {
        self.mcp.busy = false;
        let agent = waiting.agent.clone();
        match result {
            Ok(before) => {
                let after = match &waiting.call {
                    Call::SetSetting(set) => set.value.clone(),
                    _ => Value::Null,
                };
                if let Some(request) = self.mcp.open.get_mut(&agent) {
                    request
                        .before
                        .settings
                        .entry(key.clone())
                        .or_insert(before.clone());
                    if !request.summary.settings.contains(&key) {
                        request.summary.settings.push(key.clone());
                    }
                }
                let before = before.as_ref().map(json_of).unwrap_or(Value::Null);
                // A text's value is not journaled: it may be a path.
                let what = if after.is_string() {
                    key.clone()
                } else {
                    format!("{key} = {after}")
                };
                self.mcp_journal(Entry {
                    at: clock(true),
                    agent: agent.clone(),
                    tool: "set_setting",
                    what,
                    intent: Some(waiting.intent.clone()),
                    refused: None,
                });
                self.mcp_answer(
                    waiting.session,
                    waiting.id,
                    Ok(json!({ "key": key, "before": before, "after": after })),
                );
            }
            Err(why) => self.mcp_answer(waiting.session, waiting.id, Err(why)),
        }
        self.mcp_after(&agent);
        self.mcp_next();
    }

    /// After an action of `agent`'s: its row says it works on while its
    /// request is open.
    fn mcp_after(&mut self, agent: &str) {
        if !self.mcp.open.contains_key(agent)
            && let Some(seen) = self.mcp.seen.iter_mut().find(|s| s.name == agent)
            && matches!(seen.doing, McpDoing::Acting(_))
        {
            seen.doing = McpDoing::Ready;
        }
    }

    /// `agent`'s request settles: its toast, its journal's line.
    fn mcp_settle(&mut self, agent: &str) {
        // An action still shown or written for it: later.
        let pending = self
            .mcp
            .showing
            .as_ref()
            .is_some_and(|s| s.waiting.agent == agent)
            || self.mcp.queue.iter().any(|w| w.agent == agent)
            || self.mcp.busy;
        if pending {
            let intent = self
                .mcp
                .open
                .get(agent)
                .map(|r| r.intent.clone())
                .unwrap_or_default();
            let kind = self
                .mcp
                .open
                .get(agent)
                .map_or(McpKind::Windows, |r| r.kind);
            if let Some(request) = self.mcp.open.get_mut(agent) {
                request.settle = None;
            }
            return self.mcp_request(agent, &intent, kind);
        }
        let Some(mut request) = self.mcp.open.remove(agent) else {
            return;
        };
        request.settle = None;
        let at = clock(false);
        self.mcp_doing(
            agent,
            McpDoing::Did {
                summary: request.summary.clone(),
                at,
            },
        );
        let undo = request.before.undoable();
        if !request.summary.is_empty() {
            self.mcp_line_shown(
                agent,
                McpSaid::Did {
                    summary: request.summary.clone(),
                    desktop: request.desktop,
                    undo: undo.then_some(request.id),
                },
            );
            self.mcp_toast(&request, undo);
        }
        self.mcp.done.push_back(request);
        while self.mcp.done.len() > REQUESTS {
            self.mcp.done.pop_front();
        }
    }

    fn mcp_toast(&mut self, request: &Request, undo: bool) {
        let Ok(output) = self.mcp_output(None) else {
            return;
        };
        let (Some(area), Some(geometry)) =
            (self.work_area(&output), self.space.output_geometry(&output))
        else {
            return;
        };
        if let Some((_, timer)) = self.mcp.toast.take() {
            self.loop_handle.remove(timer);
        }
        let toast = McpToast {
            request: request.id,
            agent: request.agent.clone(),
            summary: request.summary.clone(),
            intent: request.intent.clone(),
            undo,
            output: output.name(),
            left: area.x - geometry.loc.x + TOAST_IN,
            bottom: area.y - geometry.loc.y + area.height - TOAST_IN,
            since: crate::panel::micros(Duration::from(self.clock.now())),
        };
        let id = request.id;
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(MCP_TOAST), move |_, _, app| {
                    if app
                        .state
                        .mcp
                        .toast
                        .as_ref()
                        .is_some_and(|(t, _)| t.request == id)
                    {
                        app.state.mcp.toast = None;
                    }
                    TimeoutAction::Drop
                });
        match timer {
            Ok(timer) => self.mcp.toast = Some((toast, timer)),
            Err(err) => tracing::warn!(error = %err.error, "no MCP toast"),
        }
    }

    /// "Annuler": request `id` put back as it was.
    pub(crate) fn mcp_undo(&mut self, id: u64) {
        let Some(index) = self.mcp.done.iter().position(|r| r.id == id && !r.undone) else {
            tracing::debug!(request = id, "nothing to undo");
            return;
        };
        let Some(request) = self.mcp.done.get_mut(index) else {
            return;
        };
        let before = std::mem::take(&mut request.before);
        request.undone = true;
        let (agent, summary) = (request.agent.clone(), request.summary.clone());
        tracing::info!(agent, request = id, "an MCP request undone");
        for (window, placed) in &before.windows {
            self.mcp_place(*window, *placed);
        }
        for (name, desktop) in &before.desktops {
            let output = self.space.outputs().find(|o| o.name() == *name).cloned();
            if let Some(output) = output
                && self.strip(&output).current() != *desktop
            {
                self.switch_desktop(&output, *desktop, None);
            }
        }
        if !before.settings.is_empty()
            && let Some(file) = self.config.file.clone()
        {
            let sender = self.agents.sender.clone();
            let settings = before.settings;
            crate::programs::on_a_thread("mcp settings back".to_owned(), move || {
                let failed: Vec<String> = settings
                    .into_iter()
                    .filter_map(|(key, value)| {
                        write_setting(&file, &key, value)
                            .err()
                            .map(|e| format!("{key}: {e}"))
                    })
                    .collect();
                if let Some(sender) = sender {
                    let _ = sender.send(Heard::Mcp(FromAgent::Restored { failed }));
                }
            });
        }
        if self
            .mcp
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.request == id)
            && let Some((_, timer)) = self.mcp.toast.take()
        {
            self.loop_handle.remove(timer);
        }
        for line in &mut self.mcp.lines {
            if let McpSaid::Did { undo, .. } = &mut line.said
                && *undo == Some(id)
            {
                *undo = None;
            }
        }
        self.mcp_line_shown(&agent, McpSaid::Undone { summary });
        self.mcp_journal(Entry {
            at: clock(true),
            agent,
            tool: "undo",
            what: format!("request {id}"),
            intent: None,
            refused: None,
        });
    }

    /// Window `id` put back where it was.
    fn mcp_place(&mut self, id: WindowId, placed: Placed) {
        if !self.windows.contains_key(&id) {
            return;
        }
        if self.model.desktop(id) != placed.desktop {
            self.send_window_to_desktop(id, placed.desktop, false);
        }
        match placed.mode {
            Mode::Floating => {
                if self.model.mode(id) != Mode::Floating {
                    self.model.unsnap(id);
                }
                self.start_settle(id);
                self.configure(id, placed.rect);
            }
            Mode::Snapped(zone) => {
                if self.model.mode(id) != placed.mode {
                    self.snap(id, zone);
                }
            }
            Mode::Fullscreen => self.set_fullscreen(id, true),
        }
        if placed.minimized && !self.model.is_minimized(id) {
            self.minimize(id);
        }
    }

    /// `Super+Shift+Escape`: every agent stopped, or let go on.
    pub fn mcp_toggle_stop(&mut self) {
        let stop = !self.mcp.stopped;
        self.mcp_stop(stop);
    }

    pub(crate) fn mcp_stop(&mut self, stop: bool) {
        if self.mcp.stopped == stop {
            return;
        }
        self.mcp.stopped = stop;
        tracing::info!(stopped = stop, "the MCP agents");
        if !stop {
            return;
        }
        // What waits is refused, what shows is not done.
        let mut refused: Vec<Waiting> = self.mcp.queue.drain(..).collect();
        if let Some(showing) = self.mcp.showing.take() {
            if let Some(timer) = showing.timer {
                self.loop_handle.remove(timer);
            }
            refused.insert(0, showing.waiting);
        }
        for waiting in refused {
            self.mcp_refuse(
                waiting.session,
                waiting.id,
                &waiting.agent,
                &waiting.call,
                McpRefusal::Stopped,
            );
            self.mcp_after(&waiting.agent);
        }
        self.redraw = true;
    }

    /// What the agents show now.
    pub fn mcp_shown(&self) -> McpShown {
        McpShown {
            stopped: self.mcp.stopped,
            agents: self
                .mcp
                .seen
                .iter()
                .map(|s| McpRow {
                    id: s.id,
                    name: s.name.clone(),
                    doing: s.doing.clone(),
                })
                .collect(),
            show: self.mcp.showing.as_ref().map(|s| s.show.clone()),
            toast: self.mcp.toast.as_ref().map(|(t, _)| t.clone()),
            journal: self.mcp.lines.iter().cloned().collect(),
        }
    }

    /// Tells the shell what the agents show, when it changed this turn.
    pub fn flush_mcp(&mut self) {
        let shown = self.mcp_shown();
        if self.mcp.told.as_ref() == Some(&shown) {
            return;
        }
        if self.shell_linked() {
            self.tell_shell(ToShell::Mcp(shown.clone()));
        }
        self.mcp.told = Some(shown);
    }

    /// As the shell connects.
    pub(crate) fn tell_mcp(&mut self) {
        let shown = self.mcp_shown();
        self.tell_shell(ToShell::Mcp(shown.clone()));
        self.mcp.told = Some(shown);
    }

    /// The window an action shows before it is done, ringed in the agent's
    /// colour, where it is drawn now (`visual`, output-local).
    pub(crate) fn mcp_ring<R>(
        &self,
        renderer: &mut R,
        window: WindowId,
        visual: Rectangle<f64, Logical>,
        scale: f64,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if self.mcp.showing.as_ref().and_then(|s| s.window) != Some(window) {
            return Vec::new();
        }
        let out = RING_OFFSET + RING_WIDTH;
        let radius = if self.model.mode(window) == Mode::Fullscreen {
            0.0
        } else {
            decoration::RADIUS
        };
        let agent = self.theme_choice().tokens().agent;
        let style = BoxStyle {
            fill: [0; 4],
            rim: [agent.r, agent.g, agent.b, (RING_ALPHA * 255.0) as u8],
            rim_width: (RING_WIDTH * 1000.0) as u32,
            radius: ((radius + out) * 1000.0) as u32,
        };
        let around = Rectangle::new(
            (visual.loc.x - out, visual.loc.y - out).into(),
            (visual.size.w + 2.0 * out, visual.size.h + 2.0 * out).into(),
        );
        box_elements(
            renderer,
            &self.textures,
            &self.mcp.ring_ids.0,
            style,
            around,
            scale,
            1.0,
        )
        .into_iter()
        .map(OutputElements::Frame)
        .collect()
    }
}

/// What an action is about, for its row.
fn kind_of(call: &Call) -> McpKind {
    match call {
        Call::SwitchDesktop(_) => McpKind::Desktop,
        Call::Launch(_) => McpKind::Launch,
        Call::SetSetting(_) => McpKind::Setting,
        _ => McpKind::Windows,
    }
}

/// A key's path: `appearance.theme` is `["appearance", "theme"]`.
fn path_of(key: &str) -> Vec<&str> {
    key.split('.').collect()
}

/// The value `value` gives setting `key`, of its default's type; or why
/// it cannot.
fn setting_value(key: &str, value: &Value) -> Result<toml_edit::Value, String> {
    let path = path_of(key);
    if path.iter().any(|p| p.is_empty()) {
        return Err(format!(
            "set_setting: {key:?} is no setting's key (appearance.theme)"
        ));
    }
    let section = path.first().copied().unwrap_or_default();
    if FORBIDDEN.contains(&section) {
        return Err(format!("set_setting: no agent changes [{section}]"));
    }
    let Some(default) = hocus_config::defaults::value(&path) else {
        return Err(format!(
            "set_setting: no setting {key:?}; get_settings lists those there are"
        ));
    };
    toml_of(value, default).ok_or_else(|| {
        format!(
            "set_setting: {key} takes {} (its default is {}), not {value}",
            kind_name(default),
            default.to_string().trim()
        )
    })
}

fn kind_name(value: &toml_edit::Value) -> &'static str {
    match value {
        toml_edit::Value::String(_) => "a text",
        toml_edit::Value::Integer(_) => "a whole number",
        toml_edit::Value::Float(_) => "a number",
        toml_edit::Value::Boolean(_) => "true or false",
        toml_edit::Value::Array(_) => "a list",
        _ => "a value of its own kind",
    }
}

/// `value` as TOML of `like`'s type.
fn toml_of(value: &Value, like: &toml_edit::Value) -> Option<toml_edit::Value> {
    Some(match (like, value) {
        (toml_edit::Value::String(_), Value::String(s)) => toml_edit::Value::from(s.as_str()),
        (toml_edit::Value::Integer(_), Value::Number(n)) => toml_edit::Value::from(n.as_i64()?),
        (toml_edit::Value::Float(_), Value::Number(n)) => toml_edit::Value::from(n.as_f64()?),
        (toml_edit::Value::Boolean(_), Value::Bool(b)) => toml_edit::Value::from(*b),
        (toml_edit::Value::Array(like), Value::Array(items)) => {
            let first = like.iter().next();
            let mut array = toml_edit::Array::new();
            for item in items {
                let item = match first {
                    Some(first) => toml_of(item, first)?,
                    None => match item {
                        Value::String(s) => toml_edit::Value::from(s.as_str()),
                        _ => return None,
                    },
                };
                array.push(item);
            }
            toml_edit::Value::Array(array)
        }
        _ => return None,
    })
}

/// A TOML value as JSON.
fn json_of(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(s) => json!(s.value()),
        toml_edit::Value::Integer(i) => json!(i.value()),
        toml_edit::Value::Float(f) => json!(f.value()),
        toml_edit::Value::Boolean(b) => json!(b.value()),
        toml_edit::Value::Datetime(d) => json!(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(json_of).collect()),
        toml_edit::Value::InlineTable(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.to_owned(), json_of(v))).collect())
        }
    }
}

/// Every setting the defaults list, as `text` (the file) sets it, else
/// its default: by dotted key, `section`'s alone when given.
fn settings_now(text: &str, section: Option<&str>) -> serde_json::Map<String, Value> {
    let file = hocus_config::parse(text).unwrap_or_default();
    let mut out = serde_json::Map::new();
    walk(
        hocus_config::defaults::document().as_table(),
        &mut Vec::new(),
        &mut |path, default| {
            if section.is_some_and(|s| path.first().map(String::as_str) != Some(s)) {
                return;
            }
            let keys: Vec<&str> = path.iter().map(String::as_str).collect();
            let set = value_at(file.as_table(), &keys);
            out.insert(path.join("."), json_of(set.unwrap_or(default)));
        },
    );
    out
}

fn walk(
    table: &dyn toml_edit::TableLike,
    path: &mut Vec<String>,
    each: &mut dyn FnMut(&[String], &toml_edit::Value),
) {
    for (key, item) in table.iter() {
        path.push(key.to_owned());
        match item {
            toml_edit::Item::Value(value) => each(path, value),
            item => {
                if let Some(inner) = item.as_table_like() {
                    walk(inner, path, each);
                }
            }
        }
        path.pop();
    }
}

fn value_at<'a>(
    table: &'a dyn toml_edit::TableLike,
    path: &[&str],
) -> Option<&'a toml_edit::Value> {
    let (name, tables) = path.split_last()?;
    let mut table = table;
    for t in tables {
        table = table.get(t)?.as_table_like()?;
    }
    table.get(name)?.as_value()
}

/// Setting `key` written into `file` in place (none: removed), off the
/// loop: what it was before.
fn write_setting(
    file: &std::path::Path,
    key: &str,
    value: Option<toml_edit::Value>,
) -> Result<Option<toml_edit::Value>, String> {
    let path: Vec<String> = key.split('.').map(str::to_owned).collect();
    let mut before = None;
    hocus_config::edit(
        file,
        |doc| {
            let (name, tables) = path.split_last().ok_or("no key")?;
            let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
            for t in tables {
                if table.get(t).is_none() {
                    if value.is_none() {
                        return Ok(());
                    }
                    let mut new = toml_edit::Table::new();
                    new.set_implicit(true);
                    table.insert(t, toml_edit::Item::Table(new));
                }
                table = table
                    .get_mut(t)
                    .and_then(toml_edit::Item::as_table_like_mut)
                    .ok_or_else(|| format!("{t} is not a table in the file"))?;
            }
            before = table.get(name).and_then(toml_edit::Item::as_value).cloned();
            match value {
                Some(value) => {
                    table.insert(name, toml_edit::Item::Value(value));
                }
                None => {
                    table.remove(name);
                }
            }
            Ok(())
        },
        |_| Ok(()),
    )
    .map_err(|e| e.to_string())?;
    Ok(before.map(|mut v| {
        v.decor_mut().clear();
        v
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_takes_a_value_of_its_defaults_type_and_never_the_agents() {
        assert!(setting_value("appearance.theme", &json!("light")).is_ok());
        assert!(setting_value("appearance.theme", &json!(3)).is_err());
        assert!(setting_value("idle.delay", &json!(600)).is_ok());
        assert!(setting_value("no.such", &json!(1)).is_err());
        assert!(setting_value("agents.halo", &json!("off")).is_err());
        assert!(setting_value("shortcuts.stop-agents", &json!("")).is_err());
        assert!(setting_value("appearance", &json!("x")).is_err());
    }

    #[test]
    fn settings_read_the_file_over_the_defaults() {
        let all = settings_now("[appearance]\ntheme = \"light\"\n", None);
        assert_eq!(all.get("appearance.theme"), Some(&json!("light")));
        assert_eq!(all.get("idle.delay"), Some(&json!(300)));
        let some = settings_now("", Some("idle"));
        assert!(some.keys().all(|k| k.starts_with("idle.")), "{some:?}");
    }

    #[test]
    fn a_setting_written_says_what_it_was_and_goes_back() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "# mine\n[idle]\ndelay = 120\n")?;
        let before = write_setting(&file, "appearance.theme", Some("light".into()))
            .map_err(anyhow::Error::msg)?;
        assert!(before.is_none());
        let before =
            write_setting(&file, "idle.delay", Some(600.into())).map_err(anyhow::Error::msg)?;
        assert_eq!(before.map(|v| v.to_string()), Some("120".to_owned()));
        write_setting(&file, "appearance.theme", None).map_err(anyhow::Error::msg)?;
        let text = std::fs::read_to_string(&file)?;
        assert!(text.starts_with("# mine\n[idle]\ndelay = 600\n"), "{text}");
        assert!(!text.contains("theme"), "{text}");
        Ok(())
    }
}
