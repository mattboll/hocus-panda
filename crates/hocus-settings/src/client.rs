//! The settings window as a Wayland client: the view drawn into
//! shared-memory buffers, pointer and keyboard handed to it. What is in
//! effect comes from the control API on a thread of its own, which follows
//! the event stream; changes go to the configuration file.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use hocus_api::{Client as Api, Event, Reply, Request};
use hocus_shell::Lang;
use hocus_ui::{Accent, AccessNode, Color, Key, Point, Size, Theme, ThemeName};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::{
    catalogue::{Context, Page},
    edit,
    view::{Outcome, SettingsView},
};

/// The window's size before the compositor says otherwise.
const SIZE: (u32, u32) = (1200, 820);
const BTN_LEFT: u32 = 0x110;

#[derive(Debug, Clone)]
pub struct Options {
    /// The compositor's Wayland socket.
    pub wayland: PathBuf,
    /// Its control socket.
    pub api: PathBuf,
    pub page: Option<Page>,
    pub lang: Lang,
    pub app_id: String,
}

/// The app running on a thread of its own (tests).
#[derive(Debug)]
pub struct Running {
    access: Arc<Mutex<Option<AccessNode>>>,
    thread: JoinHandle<anyhow::Result<()>>,
}

impl Running {
    /// What the window shows, as screen readers read it, after its last
    /// frame.
    pub fn access(&self) -> Option<AccessNode> {
        self.access.lock().ok().and_then(|a| a.clone())
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub fn join(self) -> anyhow::Result<()> {
        self.thread
            .join()
            .map_err(|_| anyhow::anyhow!("the settings app panicked"))?
    }
}

pub fn spawn(options: Options) -> anyhow::Result<Running> {
    let access = Arc::new(Mutex::new(None));
    let thread = std::thread::Builder::new().name("settings".into()).spawn({
        let access = access.clone();
        move || run(options, Some(access))
    })?;
    Ok(Running { access, thread })
}

/// What the control API thread tells the window.
enum FromApi {
    /// What is in effect: the configuration, the screens, the accent
    /// (which may come from the wallpaper) and the file.
    Context(Box<(Context, Option<String>, Option<PathBuf>)>),
    Open(Option<String>),
    Gone(String),
}

fn api_thread(path: PathBuf, to_window: Sender<FromApi>) {
    let result = (|| -> anyhow::Result<()> {
        let mut requests = Api::connect(&path)?;
        let mut events = Api::connect(&path)?;
        events.request(Request::Subscribe)?;
        let fetch = |requests: &mut Api| -> anyhow::Result<()> {
            let Reply::Config(info) = requests.request(Request::Config)? else {
                anyhow::bail!("no configuration");
            };
            let Reply::Outputs(outputs) = requests.request(Request::Outputs)? else {
                anyhow::bail!("no outputs");
            };
            let config = hocus_core::Config::parse(&info.toml)
                .map_err(|e| anyhow::anyhow!("the configuration in effect: {e}"))?;
            let ctx = Context { config, outputs };
            let path = info.path.map(PathBuf::from);
            let sent = to_window.send(FromApi::Context(Box::new((ctx, info.accent, path))));
            sent.map_err(|_| anyhow::anyhow!("the window is gone"))
        };
        fetch(&mut requests)?;
        loop {
            match events.next_event()? {
                Event::ConfigApplied | Event::OutputAdded { .. } | Event::OutputRemoved { .. } => {
                    fetch(&mut requests)?;
                }
                Event::OpenSettings { page } => {
                    if to_window.send(FromApi::Open(page)).is_err() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    })();
    if let Err(err) = result {
        let _ = to_window.send(FromApi::Gone(format!("{err:#}")));
    }
}

/// The theme the configuration asks for, as the compositor makes it.
pub fn theme(ctx: &Context, accent: Option<&str>) -> Theme {
    let a = &ctx.config.appearance;
    let mut theme = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark));
    if let Ok(named) = a.accent.parse::<Accent>() {
        theme = theme.with_accent(named);
    } else if a.accent == "wallpaper"
        && let Some([r, g, b]) = accent.and_then(hocus_core::config::parse_hex)
    {
        theme = theme.with_custom_accent(Color::rgb(r, g, b));
    }
    if a.high_contrast {
        theme = theme.with_high_contrast();
    }
    theme
        .with_text_size(a.text_size)
        .with_reduced_motion(a.reduced_motion)
}

pub fn run(options: Options, access: Option<Arc<Mutex<Option<AccessNode>>>>) -> anyhow::Result<()> {
    let stream = std::os::unix::net::UnixStream::connect(&options.wayland)
        .with_context(|| format!("could not connect to {}", options.wayland.display()))?;
    let conn = Connection::from_socket(stream)?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;

    let surface = compositor.create_surface(&qh);
    // The compositor draws the corners, the shadow and the ring.
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_app_id(options.app_id.clone());
    window.set_title(crate::words::SETTINGS.get(options.lang));
    window.set_min_size(Some((720, 480)));
    window.commit();

    let (to_window, from_api) = channel();
    std::thread::Builder::new()
        .name("settings api".into())
        .spawn({
            let path = options.api.clone();
            move || api_thread(path, to_window)
        })?;
    // The first frame waits for what is in effect.
    let deadline = Instant::now() + Duration::from_secs(5);
    let (ctx, accent, file) = loop {
        match from_api.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(FromApi::Context(what)) => break *what,
            Ok(FromApi::Gone(err)) => anyhow::bail!("the compositor did not answer: {err}"),
            Ok(FromApi::Open(_)) => {}
            Err(_) => anyhow::bail!("the compositor did not answer in time"),
        }
    };
    let theme = theme(&ctx, accent.as_deref());
    let mut view = SettingsView::new(
        theme,
        options.lang,
        Size::new(SIZE.0 as f32, SIZE.1 as f32),
        1.0,
        ctx,
    );
    view.open(options.page);

    let pool = SlotPool::new((SIZE.0 * SIZE.1 * 4) as usize, &shm)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        view,
        accent,
        file,
        size: None,
        scale: 1,
        configured: false,
        frame_pending: false,
        needs_draw: true,
        keyboard: None,
        pointer: None,
        closed: false,
        started: Instant::now(),
        access,
        from_api,
    };
    while !app.closed {
        app.take_api_news();
        if app.configured && app.needs_draw && !app.frame_pending {
            app.draw(&qh)?;
        }
        queue.flush()?;
        queue.dispatch_pending(&mut app)?;
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let wait = if app.needs_draw {
                8_000_000
            } else {
                30_000_000
            };
            let timeout = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: wait,
            };
            if rustix::event::poll(&mut fds, Some(&timeout))? > 0 {
                guard.read()?;
            }
        }
    }
    Ok(())
}

struct App {
    registry: RegistryState,
    seat: SeatState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    view: SettingsView,
    accent: Option<String>,
    file: Option<PathBuf>,
    /// The size the compositor gave, logical pixels.
    size: Option<(u32, u32)>,
    scale: i32,
    configured: bool,
    frame_pending: bool,
    needs_draw: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    closed: bool,
    started: Instant,
    access: Option<Arc<Mutex<Option<AccessNode>>>>,
    from_api: Receiver<FromApi>,
}

impl App {
    fn take_api_news(&mut self) {
        while let Ok(message) = self.from_api.try_recv() {
            match message {
                FromApi::Context(what) => {
                    let (ctx, accent, file) = *what;
                    self.view.set_theme(theme(&ctx, accent.as_deref()));
                    self.view.set_context(ctx);
                    self.accent = accent;
                    self.file = file;
                }
                FromApi::Open(page) => {
                    self.view.open(page.as_deref().and_then(Page::from_slug));
                }
                FromApi::Gone(err) => {
                    tracing::warn!(err, "the compositor went away");
                    self.closed = true;
                }
            }
            self.needs_draw = true;
        }
    }

    fn outcomes(&mut self, outcomes: Vec<Outcome>) {
        for outcome in outcomes {
            match outcome {
                Outcome::Close => self.closed = true,
                Outcome::Change(change) => {
                    let Some(file) = &self.file else {
                        tracing::warn!("no configuration file to change");
                        continue;
                    };
                    if let Err(err) = edit::write(file, &change) {
                        tracing::warn!(%err, key = change.key, "could not change the setting");
                    }
                }
            }
        }
        self.needs_draw = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) -> anyhow::Result<()> {
        let (w, h) = self.size.unwrap_or(SIZE);
        let scale = self.scale.max(1);
        self.view
            .resize(Size::new(w as f32, h as f32), scale as f32);
        let now = self.started.elapsed();
        let damage = self.view.draw(now);
        let (pw, ph) = self.view.pixel_size();
        let (buffer, canvas) = self.pool.create_buffer(
            pw as i32,
            ph as i32,
            pw as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        // Premultiplied RGBA to ARGB8888, little endian.
        for (out, px) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.view.pixels().as_chunks::<4>().0)
        {
            *out = [px[2], px[1], px[0], px[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(scale);
        // A new buffer each time: all of it is new to the compositor.
        let _ = damage;
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        buffer.attach_to(surface)?;
        self.needs_draw = self.view.is_animating();
        if self.needs_draw {
            surface.frame(qh, surface.clone());
            self.frame_pending = true;
        }
        self.window.commit();
        if let Some(access) = &self.access
            && let Ok(mut slot) = access.lock()
        {
            *slot = Some(self.view.access());
        }
        Ok(())
    }
}

fn key_of(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        Keysym::Escape => Key::Escape,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Tab => Key::Tab,
        Keysym::ISO_Left_Tab => Key::BackTab,
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete => Key::Delete,
        _ => return None,
    })
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.closed = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        self.size = match (w, h) {
            (Some(w), Some(h)) => Some((w.get(), h.get())),
            // Its own size, within what the screen leaves.
            _ => self.size.or_else(|| {
                let (bw, bh) = configure.suggested_bounds.unwrap_or(SIZE);
                let fit = |want: u32, room: u32| {
                    if room == 0 {
                        want
                    } else {
                        want.min(room.saturating_sub(48))
                    }
                };
                Some((fit(SIZE.0, bw).max(720), fit(SIZE.1, bh).max(480)))
            }),
        };
        self.configured = true;
        self.needs_draw = true;
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if factor != self.scale {
            self.scale = factor;
            self.needs_draw = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat.get_keyboard(qh, &seat, None) {
                Ok(k) => self.keyboard = Some(k),
                Err(err) => tracing::warn!(%err, "no keyboard"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat.get_pointer(qh, &seat) {
                Ok(p) => self.pointer = Some(p),
                Err(err) => tracing::warn!(%err, "no pointer"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            let at = Point::new(event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.view.pointer_move(at);
                }
                PointerEventKind::Leave { .. } => self.view.pointer_leave(),
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.view.pointer_move(at);
                    let out = self.view.pointer_button(true);
                    self.outcomes(out);
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    let out = self.view.pointer_button(false);
                    self.outcomes(out);
                }
                PointerEventKind::Axis { vertical, .. } => {
                    self.view.scroll(vertical.absolute as f32);
                }
                _ => {}
            }
            self.needs_draw = true;
        }
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        let out = match key_of(event.keysym) {
            Some(key) => self.view.key(key),
            None => match event.utf8.as_deref() {
                Some(text) if text.chars().all(|c| !c.is_control()) => self.view.text(text),
                _ => Vec::new(),
            },
        };
        self.outcomes(out);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: u32,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_registry!(App);
