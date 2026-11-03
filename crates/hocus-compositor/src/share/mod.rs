//! Screen sharing (5.11, docs/design/sharing.md): a whole screen or a
//! window, for video calls and remote tools, through the desktop portal.
//!
//! - **The portal** ([`portal`]): the desktop serves
//!   `org.freedesktop.impl.portal.ScreenCast` on the session bus, on a
//!   thread of its own; xdg-desktop-portal calls it for the applications.
//!   A `Start` comes to the loop, owned, and waits there for its answer.
//! - **The chooser** is the shell's (`share-choose`): screens and windows,
//!   every desktop's and the minimised ones too, with live thumbnails that
//!   the compositor draws over the shell's surface where it says they are
//!   (`share thumbs`), at most twice a second: drawn again at their own
//!   resolution and made small on a thread ([`thumbs`]). The compositor
//!   darkens the screen under it, draws its shadow, pops it in, and keeps
//!   the pointer from what is under the darkening.
//! - **The stream** ([`stream`]): a PipeWire video source per share, on a
//!   thread of its own. The loop draws a frame when something changed and
//!   the consumer's pace allows ([`frames`]), as the screen shows it but
//!   for what only the screen shows (the chooser, the capture's drawing,
//!   the ring) and a password manager's windows black; it starts reading
//!   it back, takes the pixels at its next turn, and hands them over.
//! - **While sharing**: the shared window, or screen, has a ring that
//!   breathes ([`ring`]), never in the stream; the shell shows the bar's
//!   sign (`shares`) and holds the notifications. Stopping from the bar,
//!   the window closing, the screen unplugged, or the application closing
//!   the session ends the share.

mod draw;
pub mod frames;
mod portal;
pub mod ring;
mod stream;
mod thumbs;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::Duration,
};

use hocus_link::{
    Rect, SHARE_MONITOR, SHARE_THUMBS_EVERY, SHARE_WINDOW, ShareAsk, ShareChoose, ShareScreen,
    ShareShown, ShareSource, ShareWindow, SharedThing, ToShell,
};
use smithay::{
    backend::renderer::element::{Id, memory::MemoryRenderBuffer},
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        channel::{self, Event},
        timer::{TimeoutAction, Timer},
    },
};
use tokio::sync::oneshot;

pub use draw::Drawn;
use portal::{Answer, FromPortal, Portal, StartAsk, StreamOut};
use stream::{FromStreams, Streams, ToStreams};

use crate::{State, model::WindowId, shell::Bus};

/// After drawing a frame, its pixels are taken this much later: the GPU
/// has drawn and copied them by then, and the loop never waits for it.
const READ_BACK: Duration = Duration::from_millis(4);
/// Pixels' memory kept to draw the next frames into.
const SPARE_KEPT: usize = 4;
/// A stream that streams gets a frame at least this often, whatever
/// changed: a consumer that joined, or one whose first frames were lost
/// while it was being linked, gets a picture of a still screen.
const KEEP: Duration = Duration::from_secs(1);

/// A request to share, waiting for the chooser or in it.
#[derive(Debug)]
struct Ask {
    id: u64,
    session: String,
    /// The asking application's id (empty: not known) and its window's
    /// title.
    app: String,
    detail: String,
    types: u32,
    pointer: bool,
    reply: Option<oneshot::Sender<Answer>>,
}

/// The chooser shown.
#[derive(Debug)]
struct Choosing {
    ask: Ask,
    output: String,
    /// Its surface since its first frame.
    surface: Option<(LayerSurface, Duration)>,
    /// Where its thumbnails go on its surface (logical pixels).
    rects: Vec<(ShareSource, Rect)>,
}

/// A share: made, then live once its stream is on PipeWire.
#[derive(Debug)]
pub(crate) struct Active {
    pub id: u64,
    session: String,
    pub source: ShareSource,
    pub pointer: bool,
    app: String,
    /// What the bar says it shares.
    thing: SharedThing,
    pub since: Duration,
    /// Its PipeWire node, once on the graph; the portal's answer until.
    pub node: Option<u32>,
    reply: Option<oneshot::Sender<Answer>>,
    /// The size offered, and the one agreed with its pace.
    offered: (u32, u32),
    pub agreed: Option<((u32, u32), u32)>,
    pub streaming: bool,
    /// When its last frame was drawn; a frame is owed whatever changed.
    last: Option<Duration>,
    fresh: bool,
    /// The plain ring's pieces, without shaders.
    pub ring_ids: [Id; 9],
}

/// Screen sharing, owned by the loop.
#[derive(Debug, Default)]
pub struct Sharing {
    portal: Option<Portal>,
    streams: Option<Streams>,
    maker: Option<thumbs::Maker>,
    choosing: Option<Choosing>,
    waiting: VecDeque<Ask>,
    pub(crate) shares: BTreeMap<u64, Active>,
    next: u64,
    /// The thumbnails made, by what they show, and their size in pixels.
    pictures: HashMap<ShareSource, (MemoryRenderBuffer, (u32, u32))>,
    /// The thumbnails are to be drawn again, and the timer that says so.
    thumbs_due: bool,
    thumbs_timer: Option<RegistrationToken>,
    /// The rings' breath, and the streams' pace.
    breath: Option<RegistrationToken>,
    pace: Option<RegistrationToken>,
    reading: Option<RegistrationToken>,
    /// While a consumer streams, a frame at least every [`KEEP`].
    keep: Option<RegistrationToken>,
    /// A stream waited for its pace: a frame is owed.
    streams_due: bool,
    spare: Vec<Vec<u8>>,
    pub(crate) drawn: Drawn,
    /// Frames handed to PipeWire, for tests.
    frames_sent: u64,
}

/// What a test can know of a share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareInfo {
    pub id: u64,
    pub source: ShareSource,
    pub pointer: bool,
    pub node: Option<u32>,
    pub streaming: bool,
}

impl Sharing {
    /// The shares under way, for tests.
    pub fn info(&self) -> Vec<ShareInfo> {
        self.shares
            .values()
            .map(|s| ShareInfo {
                id: s.id,
                source: s.source.clone(),
                pointer: s.pointer,
                node: s.node,
                streaming: s.streaming,
            })
            .collect()
    }

    /// The request whose chooser shows, for tests.
    pub fn choosing(&self) -> Option<u64> {
        self.choosing.as_ref().map(|c| c.ask.id)
    }

    /// The frames handed to PipeWire so far, for tests.
    pub fn frames_sent(&self) -> u64 {
        self.frames_sent
    }

    /// Whether `id` is shared.
    pub fn window_shared(&self, id: WindowId) -> bool {
        self.shares
            .values()
            .any(|s| s.node.is_some() && s.source == ShareSource::Window(id.0))
    }

    /// Whether screen `name` is shared.
    pub fn screen_shared(&self, name: &str) -> bool {
        self.shares
            .values()
            .any(|s| s.node.is_some() && matches!(&s.source, ShareSource::Screen(n) if n == name))
    }

    /// Pixels' memory of `len` bytes to draw a frame into.
    fn spare_buffer(&mut self, len: usize) -> Vec<u8> {
        match self.spare.iter().position(|b| b.capacity() >= len) {
            Some(at) => self.spare.swap_remove(at),
            None => Vec::with_capacity(len),
        }
    }
}

impl State {
    /// Serves the screen cast portal on the session bus the shell serves
    /// the notifications on, if there is one; with none (nested or
    /// headless without `--session-bus`), nothing is.
    pub fn start_sharing(&mut self) {
        let Some(bus) = self.shell.bus.clone() else {
            tracing::info!("no session bus: no screen cast portal");
            return;
        };
        let word = match bus {
            Bus::Given(address) => address,
            Bus::Session => "session".to_owned(),
        };
        let (sender, receiver) = channel::channel::<FromPortal>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.portal_says(said);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "could not listen to the screen cast portal");
            return;
        }
        self.share.portal = portal::start(word, sender);
    }

    fn portal_says(&mut self, said: FromPortal) {
        match said {
            FromPortal::Start(ask) => self.share_start(ask),
            FromPortal::Cancel { session } => {
                if self
                    .share
                    .choosing
                    .as_ref()
                    .is_some_and(|c| c.ask.session == session)
                {
                    self.end_chooser(Some(Answer::Cancelled));
                } else {
                    self.share.waiting.retain(|a| a.session != session);
                }
            }
            FromPortal::Closed { session } => {
                if self
                    .share
                    .choosing
                    .as_ref()
                    .is_some_and(|c| c.ask.session == session)
                {
                    self.end_chooser(Some(Answer::Cancelled));
                }
                self.share.waiting.retain(|a| a.session != session);
                let ids: Vec<u64> = self
                    .share
                    .shares
                    .values()
                    .filter(|s| s.session == session)
                    .map(|s| s.id)
                    .collect();
                for id in ids {
                    self.end_share(id, false);
                }
            }
        }
    }

    /// A `Start`: the chooser opens for it once the one before it is done.
    fn share_start(&mut self, ask: StartAsk) {
        self.share.next += 1;
        let id = self.share.next;
        let (app, detail) = self.asking_app(&ask.app_id, &ask.parent_window);
        let mut types = ask.types & (SHARE_MONITOR | SHARE_WINDOW);
        if types == 0 {
            types = SHARE_MONITOR;
        }
        // Shown by default, unless the application asked for it hidden.
        let pointer = ask.cursor_mode != Some(1);
        tracing::info!(id, app, types, pointer, "a request to share the screen");
        let ask = Ask {
            id,
            session: ask.session,
            app,
            detail,
            types,
            pointer,
            reply: Some(ask.reply),
        };
        if !self.shell_linked() {
            tracing::warn!("no shell to choose what to share: refused");
            if let Some(reply) = ask.reply {
                let _ = reply.send(Answer::Failed);
            }
            return;
        }
        self.share.waiting.push_back(ask);
        self.share_next();
    }

    /// The asking application and what it says of itself: its window, by
    /// the portal's parent window (an X11 one), else by its id, else the
    /// focused one (the user just asked it to share).
    fn asking_app(&self, app_id: &str, parent: &str) -> (String, String) {
        let by_x11 = parent
            .strip_prefix("x11:")
            .and_then(|xid| u32::from_str_radix(xid.trim_start_matches("0x"), 16).ok())
            .and_then(|xid| {
                self.windows.iter().find_map(|(id, m)| {
                    m.window
                        .x11_surface()
                        .filter(|x| x.window_id() == xid)
                        .map(|_| *id)
                })
            });
        let app_of = |id: WindowId| {
            self.windows
                .get(&id)
                .and_then(|m| crate::windows::app_id(&m.window))
                .unwrap_or_default()
        };
        let same_app = |id: WindowId| {
            let app = app_of(id).to_lowercase();
            let wanted = app_id.to_lowercase();
            !wanted.is_empty()
                && (app == wanted
                    || wanted.rsplit('.').next() == Some(app.as_str())
                    || app.rsplit('.').next() == wanted.rsplit('.').next())
        };
        let focused = self.model.focused();
        let window = by_x11
            .or_else(|| focused.filter(|id| same_app(*id)))
            .or_else(|| self.model.recent().iter().copied().find(|id| same_app(*id)))
            .or_else(|| focused.filter(|_| app_id.is_empty()));
        match window {
            Some(id) => {
                let app = if app_id.is_empty() {
                    app_of(id)
                } else {
                    app_id.to_owned()
                };
                (app, crate::windows::title_of(self, id))
            }
            None => (app_id.to_owned(), String::new()),
        }
    }

    /// The next request waiting shows its chooser, if none shows.
    fn share_next(&mut self) {
        if self.share.choosing.is_some() {
            return;
        }
        let Some(ask) = self.share.waiting.pop_front() else {
            return;
        };
        let Some(output) = self.output_under_pointer() else {
            if let Some(reply) = ask.reply {
                let _ = reply.send(Answer::Failed);
            }
            return;
        };
        let output = output.name();
        self.share.choosing = Some(Choosing {
            ask,
            output,
            surface: None,
            rects: Vec::new(),
        });
        self.tell_chooser();
        self.arm_thumbs();
    }

    /// The chooser's line: its request, its screen and what it offers.
    fn tell_chooser(&mut self) {
        let Some(choosing) = &self.share.choosing else {
            return;
        };
        let (width, height) = self
            .logical_size(&choosing.output)
            .unwrap_or((1920.0, 1080.0));
        let line = ShareChoose {
            request: choosing.ask.id,
            output: choosing.output.clone(),
            scale: self.bar_scale(&choosing.output),
            width: width as i32,
            height: height as i32,
            types: choosing.ask.types,
            pointer: choosing.ask.pointer,
            app: choosing.ask.app.clone(),
            detail: choosing.ask.detail.clone(),
            screens: self.share_screens(),
            windows: self.share_windows(),
        };
        self.tell_shell(ToShell::ShareChoose(line));
    }

    fn share_screens(&self) -> Vec<ShareScreen> {
        self.space
            .outputs()
            .filter_map(|o| {
                let mode = o.current_mode()?;
                let size = o.current_transform().transform_size(mode.size);
                let model = o
                    .user_data()
                    .get::<crate::panel::ScreenModel>()
                    .map(|m| m.0.clone())
                    .unwrap_or_default();
                Some(ShareScreen {
                    name: o.name(),
                    width: size.w.unsigned_abs(),
                    height: size.h.unsigned_abs(),
                    builtin: crate::panel::built_in(&o.name()),
                    model,
                })
            })
            .collect()
    }

    /// Every window, the most recently used first: every desktop's, the
    /// minimised ones too.
    fn share_windows(&self) -> Vec<ShareWindow> {
        let mut order: Vec<WindowId> = self
            .model
            .recent()
            .iter()
            .copied()
            .filter(|id| self.windows.contains_key(id))
            .collect();
        for id in self.windows.keys() {
            if !order.contains(id) {
                order.push(*id);
            }
        }
        order
            .into_iter()
            .map(|id| {
                let desktop = self.model.desktop(id);
                let here = self
                    .output_of(id)
                    .is_none_or(|o| self.strip(&o).current() == desktop);
                ShareWindow {
                    id: id.0,
                    desktop: u8::try_from(desktop + 1).unwrap_or(u8::MAX),
                    here,
                    minimized: self.model.is_minimized(id),
                    app: self
                        .windows
                        .get(&id)
                        .and_then(|m| crate::windows::app_id(&m.window))
                        .unwrap_or_default(),
                    title: crate::windows::title_of(self, id),
                }
            })
            .collect()
    }

    /// Windows came or went while the chooser shows: it lists them again.
    pub fn share_sources_changed(&mut self) {
        if self.share.choosing.is_some() {
            self.tell_chooser();
        }
    }

    /// The thumbnails are drawn again every [`SHARE_THUMBS_EVERY`] while
    /// the chooser shows.
    fn arm_thumbs(&mut self) {
        self.share.thumbs_due = true;
        if self.share.thumbs_timer.is_some() {
            return;
        }
        let timer = Timer::from_duration(SHARE_THUMBS_EVERY);
        let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
            let share = &mut app.state.share;
            if share.choosing.is_none() {
                share.thumbs_timer = None;
                return TimeoutAction::Drop;
            }
            share.thumbs_due = true;
            TimeoutAction::ToDuration(SHARE_THUMBS_EVERY)
        });
        match inserted {
            Ok(token) => self.share.thumbs_timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "the chooser's thumbnails stay still"),
        }
    }

    /// The chooser goes, answering its request with `answer` (none: it was
    /// chosen, and answers once its stream is there).
    fn end_chooser(&mut self, answer: Option<Answer>) {
        let Some(mut choosing) = self.share.choosing.take() else {
            return;
        };
        if let Some(answer) = answer
            && let Some(reply) = choosing.ask.reply.take()
        {
            let _ = reply.send(answer);
        }
        if let Some(token) = self.share.thumbs_timer.take() {
            self.loop_handle.remove(token);
        }
        self.share.pictures.clear();
        self.tell_shell(ToShell::ShareChooseEnd(choosing.ask.id));
        self.redraw = true;
        self.share_next();
    }

    /// The shell says something of sharing.
    pub fn share_asked(&mut self, ask: ShareAsk) {
        let current = self.share.choosing.as_ref().map(|c| c.ask.id);
        match ask {
            ShareAsk::Thumbs { request, rects } if current == Some(request) => {
                if let Some(choosing) = &mut self.share.choosing {
                    choosing.rects = rects;
                }
                self.share.thumbs_due = true;
                self.redraw = true;
            }
            ShareAsk::Chosen {
                request,
                source,
                pointer,
            } if current == Some(request) => self.share_chosen(source, pointer),
            ShareAsk::Cancel { request } if current == Some(request) => {
                tracing::info!(request, "sharing cancelled");
                self.end_chooser(Some(Answer::Cancelled));
            }
            ShareAsk::Stop { id } => {
                tracing::info!(id, "sharing stopped from the bar");
                self.end_share(id, true);
            }
            other => tracing::debug!(?other, "a share's ask for a chooser gone"),
        }
    }

    /// The size of what `source` is, in pixels, and what the bar says of
    /// it; `None`: it is not there.
    fn source_size(&self, source: &ShareSource) -> Option<((u32, u32), SharedThing)> {
        match source {
            ShareSource::Screen(name) => {
                let o = self.space.outputs().find(|o| o.name() == *name)?;
                let mode = o.current_mode()?;
                let size = o.current_transform().transform_size(mode.size);
                let label = if crate::panel::built_in(name) {
                    String::new()
                } else {
                    o.user_data()
                        .get::<crate::panel::ScreenModel>()
                        .map_or_else(|| name.clone(), |m| m.0.clone())
                };
                Some((
                    (size.w.unsigned_abs(), size.h.unsigned_abs()),
                    SharedThing::Screen(label),
                ))
            }
            ShareSource::Window(id) => {
                let id = WindowId(*id);
                let rect = self.rect_of(id)?;
                let scale = self
                    .window_output(id)
                    .map_or(1.0, |o| o.current_scale().fractional_scale());
                let w = (f64::from(rect.width) * scale).round().max(1.0) as u32;
                let h = (f64::from(rect.height) * scale).round().max(1.0) as u32;
                Some((
                    (w, h),
                    SharedThing::Window(crate::windows::title_of(self, id)),
                ))
            }
        }
    }

    /// What to share is chosen: its stream is made, the portal answered
    /// once it is on PipeWire.
    fn share_chosen(&mut self, source: ShareSource, pointer: bool) {
        let Some((size, thing)) = self.source_size(&source) else {
            tracing::info!(%source, "what was chosen to share is gone");
            self.end_chooser(Some(Answer::Failed));
            return;
        };
        let Some(mut choosing) = self.share.choosing.take() else {
            return;
        };
        let reply = choosing.ask.reply.take();
        let id = choosing.ask.id;
        tracing::info!(id, %source, pointer, "sharing chosen");
        let name = match &thing {
            SharedThing::Screen(label) if label.is_empty() => "Hocuspanda: screen".to_owned(),
            SharedThing::Screen(label) | SharedThing::Window(label) => {
                format!("Hocuspanda: {label}")
            }
        };
        self.share.shares.insert(
            id,
            Active {
                id,
                session: choosing.ask.session.clone(),
                source,
                pointer,
                app: choosing.ask.app.clone(),
                thing,
                since: self.now(),
                node: None,
                reply,
                offered: size,
                agreed: None,
                streaming: false,
                last: None,
                fresh: true,
                ring_ids: std::array::from_fn(|_| Id::new()),
            },
        );
        self.share.choosing = Some(choosing);
        self.end_chooser(None);
        if self.share.streams.is_none() {
            self.start_streams();
        }
        match &self.share.streams {
            Some(streams) => streams.send(ToStreams::Create { id, size, name }),
            None => self.share_failed(id, "no streams' thread"),
        }
    }

    fn start_streams(&mut self) {
        let (sender, receiver) = channel::channel::<FromStreams>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.streams_say(said);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "could not listen to the streams' thread");
            return;
        }
        self.share.streams = stream::start(self.config.pipewire_remote.clone(), sender);
    }

    fn streams_say(&mut self, said: FromStreams) {
        match said {
            FromStreams::Ready { id, node } => {
                let now = self.now();
                let Some(share) = self.share.shares.get_mut(&id) else {
                    return;
                };
                share.node = Some(node);
                share.since = now;
                tracing::info!(id, node, source = %share.source, "sharing");
                let out = self.stream_out(id, node);
                if let Some(share) = self.share.shares.get_mut(&id)
                    && let Some(reply) = share.reply.take()
                {
                    let _ = reply.send(Answer::Streams(out.into_iter().collect()));
                }
                self.tell_shares();
                self.arm_breath();
                self.redraw = true;
            }
            FromStreams::Failed { id: 0, why } => {
                let ids: Vec<u64> = self.share.shares.keys().copied().collect();
                for id in ids {
                    self.share_failed(id, &why);
                }
                self.share.streams = None;
            }
            FromStreams::Failed { id, why } => self.share_failed(id, &why),
            FromStreams::Format { id, size, fps } => {
                if let Some(share) = self.share.shares.get_mut(&id) {
                    share.agreed = Some((size, fps));
                    share.fresh = true;
                    self.share.streams_due = true;
                }
            }
            FromStreams::Streaming { id, on } => {
                if let Some(share) = self.share.shares.get_mut(&id) {
                    tracing::debug!(id, on, "a share's consumer");
                    share.streaming = on;
                    share.fresh |= on;
                    self.share.streams_due = true;
                    self.redraw = true;
                }
                if on {
                    self.keep_streams();
                }
            }
            FromStreams::Dropped { id } => {
                if let Some(share) = self.share.shares.get_mut(&id) {
                    share.fresh = true;
                    let every = share.agreed.map_or(Duration::from_millis(33), |(_, fps)| {
                        Duration::from_micros(1_000_000 / u64::from(fps.max(1)))
                    });
                    self.pace_streams(every);
                }
            }
            FromStreams::Spare(pixels) => {
                if self.share.spare.len() < SPARE_KEPT {
                    self.share.spare.push(pixels);
                }
            }
        }
    }

    /// What `Start` says of share `id`'s stream.
    fn stream_out(&self, id: u64, node: u32) -> Option<StreamOut> {
        let share = self.share.shares.get(&id)?;
        Some(match &share.source {
            ShareSource::Screen(name) => {
                let o = self.space.outputs().find(|o| o.name() == *name)?;
                let g = self.space.output_geometry(o)?;
                StreamOut {
                    node,
                    position: Some((g.loc.x, g.loc.y)),
                    size: (g.size.w, g.size.h),
                    source_type: SHARE_MONITOR,
                }
            }
            ShareSource::Window(w) => {
                let rect = self.rect_of(WindowId(*w))?;
                StreamOut {
                    node,
                    position: None,
                    size: (rect.width, rect.height),
                    source_type: SHARE_WINDOW,
                }
            }
        })
    }

    /// Share `id` could not stream: its request fails, or it ends.
    fn share_failed(&mut self, id: u64, why: &str) {
        tracing::warn!(id, why, "a share could not stream");
        let starting = self.share.shares.get_mut(&id).and_then(|s| s.reply.take());
        match starting {
            Some(reply) => {
                let _ = reply.send(Answer::Failed);
                self.share.shares.remove(&id);
                if let Some(streams) = &self.share.streams {
                    streams.send(ToStreams::Destroy { id });
                }
            }
            None => self.end_share(id, true),
        }
    }

    /// Share `id` ends; `tell`: its session is closed for the application
    /// (stopped on the desktop's side).
    pub fn end_share(&mut self, id: u64, tell: bool) {
        let Some(mut share) = self.share.shares.remove(&id) else {
            return;
        };
        tracing::info!(id, source = %share.source, "sharing ends");
        if let Some(reply) = share.reply.take() {
            let _ = reply.send(Answer::Failed);
        }
        if let Some(streams) = &self.share.streams {
            streams.send(ToStreams::Destroy { id });
        }
        if tell && let Some(portal) = &self.share.portal {
            portal.close(&share.session);
        }
        self.drawn_ring_forget(id);
        self.tell_shares();
        if self.share.shares.is_empty()
            && let Some(token) = self.share.breath.take()
        {
            self.loop_handle.remove(token);
        }
        self.redraw = true;
    }

    /// Window `id` went: its shares end.
    pub fn share_window_gone(&mut self, id: WindowId) {
        let ids: Vec<u64> = self
            .share
            .shares
            .values()
            .filter(|s| s.source == ShareSource::Window(id.0))
            .map(|s| s.id)
            .collect();
        for share in ids {
            self.end_share(share, true);
        }
        self.share_sources_changed();
    }

    /// Screen `name` went: its shares end.
    pub fn share_output_gone(&mut self, name: &str) {
        let ids: Vec<u64> = self
            .share
            .shares
            .values()
            .filter(|s| matches!(&s.source, ShareSource::Screen(n) if n == name))
            .map(|s| s.id)
            .collect();
        for share in ids {
            self.end_share(share, true);
        }
        if self
            .share
            .choosing
            .as_ref()
            .is_some_and(|c| c.output == name)
        {
            self.end_chooser(Some(Answer::Cancelled));
        }
    }

    /// The bar's sign: what is shared now, by whom.
    pub fn tell_shares(&mut self) {
        let shown: Vec<ShareShown> = self
            .share
            .shares
            .values()
            .filter(|s| s.node.is_some())
            .map(|s| ShareShown {
                id: s.id,
                app: s.app.clone(),
                thing: s.thing.clone(),
            })
            .collect();
        self.tell_shell(ToShell::Shares(shown));
    }

    /// The shell connected again: what is shared, and the chooser.
    pub fn tell_sharing_again(&mut self) {
        self.tell_shares();
        self.tell_chooser();
    }

    /// The rings breathe while something is shared, in steps (none with
    /// reduced motion).
    fn arm_breath(&mut self) {
        if self.share.breath.is_some() || self.reduced_motion() {
            return;
        }
        let step = Duration::from_millis(1000 / u64::from(ring::STEPS));
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(step), move |_, _, app| {
                    let state = &mut app.state;
                    if state.share.shares.is_empty() || state.reduced_motion() {
                        state.share.breath = None;
                        return TimeoutAction::Drop;
                    }
                    state.redraw = true;
                    TimeoutAction::ToDuration(step)
                });
        match inserted {
            Ok(token) => self.share.breath = Some(token),
            Err(err) => tracing::warn!(%err.error, "the share's ring stays still"),
        }
    }

    /// A commit of the chooser's surface: its first frame starts its pop.
    pub fn share_chooser_committed(&mut self, layer: &LayerSurface) {
        let now = self.now();
        let Some(choosing) = &mut self.share.choosing else {
            return;
        };
        if choosing.surface.as_ref().is_some_and(|(l, _)| l == layer) {
            return;
        }
        let drawn = smithay::backend::renderer::utils::with_renderer_surface_state(
            layer.wl_surface(),
            |s| s.buffer().is_some(),
        )
        .unwrap_or(false);
        if drawn {
            choosing.surface = Some((layer.clone(), now));
            self.redraw = true;
        }
    }

    pub fn share_chooser_gone(&mut self, layer: &LayerSurface) {
        if let Some(choosing) = &mut self.share.choosing
            && choosing.surface.as_ref().is_some_and(|(l, _)| l == layer)
        {
            choosing.surface = None;
        }
    }

    /// The chooser's surface, on its screen, if it shows.
    fn chooser_layer(&self) -> Option<(Output, LayerSurface)> {
        let choosing = self.share.choosing.as_ref()?;
        let output = self
            .space
            .outputs()
            .find(|o| o.name() == choosing.output)?
            .clone();
        let layer = layer_map_for_output(&output)
            .layers()
            .find(|l| l.namespace() == hocus_link::SHARE_CHOOSER_NAMESPACE)
            .cloned()?;
        Some((output, layer))
    }

    /// Whether the chooser keeps the pointer at `at` from what is under
    /// it: it shows, and `at` is not on it.
    pub fn share_keeps_pointer(
        &self,
        at: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> bool {
        let Some((output, layer)) = self.chooser_layer() else {
            return false;
        };
        let Some(origin) = self.space.output_geometry(&output).map(|g| g.loc) else {
            return false;
        };
        let Some(geometry) = layer_map_for_output(&output).layer_geometry(&layer) else {
            return false;
        };
        let rect = smithay::utils::Rectangle::new(origin + geometry.loc, geometry.size).to_f64();
        !rect.contains(at)
    }

    /// A thumbnail made for the chooser.
    fn thumb_made(&mut self, made: thumbs::Made) {
        if self.share.choosing.as_ref().map(|c| c.ask.id) != Some(made.request) {
            return;
        }
        let (w, h) = made.size;
        let buffer = crate::decoration::buffer(&made.pixels, w as i32, h as i32);
        self.share.pictures.insert(made.source, (buffer, made.size));
        self.redraw = true;
    }

    /// The thread that makes the thumbnails, started the first time.
    fn thumb_maker(&mut self) -> Option<&thumbs::Maker> {
        if self.share.maker.is_none() {
            let (sender, receiver) = channel::channel::<thumbs::Made>();
            let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
                if let Event::Msg(made) = event {
                    app.state.thumb_made(made);
                }
            });
            if let Err(err) = inserted {
                tracing::warn!(%err.error, "no thumbnails for the chooser");
                return None;
            }
            self.share.maker = thumbs::Maker::start(sender);
        }
        self.share.maker.as_ref()
    }

    /// A frame of share `id`, read back, to its stream.
    fn send_frame(&mut self, id: u64, pixels: Vec<u8>, size: (u32, u32)) {
        let Some(streams) = &self.share.streams else {
            return;
        };
        self.share.frames_sent += 1;
        streams.send(ToStreams::Frame {
            id,
            pixels,
            size,
            stride: size.0 * 4,
        });
    }

    /// While a consumer streams, a frame is owed when none went for
    /// [`KEEP`]: a timer of the loop, which goes when none streams.
    fn keep_streams(&mut self) {
        if self.share.keep.is_some() {
            return;
        }
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(KEEP), |_, _, app| {
                let state = &mut app.state;
                let now = state.now();
                let mut any = false;
                for share in state.share.shares.values_mut().filter(|s| s.streaming) {
                    any = true;
                    if share.last.is_none_or(|last| now >= last + KEEP) {
                        share.fresh = true;
                        state.share.streams_due = true;
                    }
                }
                if any {
                    TimeoutAction::ToDuration(KEEP)
                } else {
                    state.share.keep = None;
                    TimeoutAction::Drop
                }
            });
        match inserted {
            Ok(token) => self.share.keep = Some(token),
            Err(err) => tracing::warn!(%err.error, "a still stream gets no frame"),
        }
    }

    /// Share `id`'s source is `size` pixels now: its stream offers it.
    fn share_resized(&mut self, id: u64, size: (u32, u32)) {
        let Some(share) = self.share.shares.get_mut(&id) else {
            return;
        };
        if share.offered == size {
            return;
        }
        share.offered = size;
        tracing::debug!(
            id,
            width = size.0,
            height = size.1,
            "a shared window's new size"
        );
        if let Some(streams) = &self.share.streams {
            streams.send(ToStreams::Resize { id, size });
        }
    }

    /// A stream's frame waits for its pace: a timer owes it, once.
    fn pace_streams(&mut self, at: Duration) {
        if self.share.pace.is_some() {
            return;
        }
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(at), |_, _, app| {
                app.state.share.pace = None;
                app.state.share.streams_due = true;
                TimeoutAction::Drop
            });
        match inserted {
            Ok(token) => self.share.pace = Some(token),
            Err(err) => tracing::warn!(%err.error, "a stream's frame not paced"),
        }
    }

    /// Frames are being read back: the loop turns again shortly to take
    /// them.
    fn read_soon(&mut self) {
        if self.share.reading.is_some() {
            return;
        }
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(READ_BACK), |_, _, app| {
                    app.state.share.reading = None;
                    TimeoutAction::Drop
                });
        match inserted {
            Ok(token) => self.share.reading = Some(token),
            Err(err) => tracing::warn!(%err.error, "a stream's frame waits"),
        }
    }
}

/// The pictures' places, for the backend's frames.
#[derive(Debug, Clone)]
pub(crate) struct ThumbAsk {
    pub request: u64,
    pub source: ShareSource,
    pub room: (u32, u32),
    pub radius: f64,
}

impl Sharing {
    /// The thumbnails to draw now, if they are due.
    pub(crate) fn take_thumbs(&mut self, scale_of: impl Fn(&str) -> f64) -> Vec<ThumbAsk> {
        if !self.thumbs_due {
            return Vec::new();
        }
        let Some(choosing) = &self.choosing else {
            self.thumbs_due = false;
            return Vec::new();
        };
        // Its first frame, its rects told: else later.
        if choosing.surface.is_none() || choosing.rects.is_empty() {
            return Vec::new();
        }
        self.thumbs_due = false;
        let scale = scale_of(&choosing.output);
        choosing
            .rects
            .iter()
            .map(|(source, r)| ThumbAsk {
                request: choosing.ask.id,
                source: source.clone(),
                room: (
                    (f64::from(r.width) * scale).round().max(1.0) as u32,
                    (f64::from(r.height) * scale).round().max(1.0) as u32,
                ),
                radius: (hocus_link::SHARE_THUMB_RADIUS - 1.0) * scale,
            })
            .collect()
    }
}
