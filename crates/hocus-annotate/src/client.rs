//! The annotation window as a Wayland client: the view drawn into
//! shared-memory buffers, pointer and keyboard handed to it. Copying goes
//! through the compositor (its control API), which keeps the clipboard
//! after the window is gone; saving writes a file next to the capture.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Instant,
};

use anyhow::Context as _;
use hocus_api::{Client as Api, Reply, Request};
use hocus_shell::Lang;
use hocus_ui::{AccessNode, Key, Point, Size, Theme};
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

use crate::editor::{self, Editor};

/// The window's size before the compositor says otherwise.
const SIZE: (u32, u32) = (1200, 820);
const BTN_LEFT: u32 = 0x110;

#[derive(Debug, Clone)]
pub struct Options {
    /// The compositor's Wayland socket.
    pub wayland: PathBuf,
    /// Its control socket, to copy.
    pub api: Option<PathBuf>,
    /// The capture, or the recording.
    pub file: PathBuf,
    pub lang: Lang,
    pub theme: Theme,
    pub editing: Editing,
}

/// Which editor the window holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    /// A screenshot's annotation (#128).
    Annotate,
    /// A recording's trim and export (#132).
    Trim,
}

/// The theme in effect, read from the compositor; the default without it.
pub fn theme_in_effect(api: Option<&PathBuf>) -> Theme {
    use hocus_ui::{Accent, ThemeName};
    let toml = api
        .and_then(|path| Api::connect(path).ok())
        .and_then(|mut c| match c.request(Request::Config).ok()? {
            Reply::Config(info) => Some(info.toml),
            _ => None,
        });
    let Some(config) = toml.and_then(|t| hocus_core::Config::parse(&t).ok()) else {
        return Theme::default();
    };
    let a = &config.appearance;
    let mut theme = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark));
    if let Ok(named) = a.accent.parse::<Accent>() {
        theme = theme.with_accent(named);
    }
    if a.high_contrast {
        theme = theme.with_high_contrast();
    }
    theme
        .with_text_size(a.text_size)
        .with_reduced_motion(a.reduced_motion)
}

/// The window running on a thread of its own (tests).
#[derive(Debug)]
pub struct Running {
    shown: Arc<Mutex<Option<Shown>>>,
    thread: JoinHandle<anyhow::Result<()>>,
}

/// What the window's last frame showed.
#[derive(Debug, Clone)]
pub struct Shown {
    /// As screen readers read it.
    pub access: AccessNode,
    /// Where the pointer was, and whether a shape was being drawn.
    pub pointer: Option<Point>,
    pub drawing: bool,
}

impl Running {
    /// What the window shows, as screen readers read it, after its last
    /// frame.
    pub fn access(&self) -> Option<AccessNode> {
        self.shown().map(|s| s.access)
    }

    /// The window's last frame.
    pub fn shown(&self) -> Option<Shown> {
        self.shown.lock().ok().and_then(|s| s.clone())
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub fn join(self) -> anyhow::Result<()> {
        self.thread
            .join()
            .map_err(|_| anyhow::anyhow!("the annotation window panicked"))?
    }
}

pub fn spawn(options: Options) -> anyhow::Result<Running> {
    let shown = Arc::new(Mutex::new(None));
    let thread = std::thread::Builder::new().name("annotate".into()).spawn({
        let shown = shown.clone();
        move || run(options, Some(shown))
    })?;
    Ok(Running { shown, thread })
}

/// A file next to `file`, never over another: ` (annotated)` and a number
/// when taken.
pub fn save_beside(file: &Path, png: &[u8]) -> std::io::Result<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Screenshot".into());
    for n in 1..1000 {
        let name = if n == 1 {
            format!("{stem} (annotated).png")
        } else {
            format!("{stem} (annotated {n}).png")
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(png)?;
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::other("no free name"))
}

pub fn run(options: Options, shown: Option<Arc<Mutex<Option<Shown>>>>) -> anyhow::Result<()> {
    let size = Size::new(SIZE.0 as f32, SIZE.1 as f32);
    let editor: Box<dyn Editor> = match options.editing {
        Editing::Annotate => Box::new(editor::Annotate::new(
            options.theme.clone(),
            options.lang,
            size,
            &options.file,
            options.api.clone(),
        )?),
        Editing::Trim => Box::new(editor::Trim::new(
            options.theme.clone(),
            options.lang,
            size,
            &options.file,
        )),
    };
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
    window.set_app_id(editor.app_id());
    window.set_title(editor.title());
    window.set_min_size(Some((640, 420)));
    window.commit();

    let pool = SlotPool::new((SIZE.0 * SIZE.1 * 4) as usize, &shm)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        editor,
        size: None,
        scale: 1,
        configured: false,
        needs_draw: true,
        keyboard: None,
        pointer: None,
        modifiers: Modifiers::default(),
        closed: false,
        started: Instant::now(),
        shown,
    };
    while !app.closed {
        if app.editor.poll() {
            app.needs_draw = true;
        }
        if app.configured && app.needs_draw {
            app.draw()?;
        }
        queue.flush()?;
        queue.dispatch_pending(&mut app)?;
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let timeout = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 30_000_000,
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
    editor: Box<dyn Editor>,
    size: Option<(u32, u32)>,
    scale: i32,
    configured: bool,
    needs_draw: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    modifiers: Modifiers,
    closed: bool,
    started: Instant,
    shown: Option<Arc<Mutex<Option<Shown>>>>,
}

impl App {
    fn draw(&mut self) -> anyhow::Result<()> {
        let (w, h) = self.size.unwrap_or(SIZE);
        let scale = self.scale.max(1);
        self.editor
            .resize(Size::new(w as f32, h as f32), scale as f32);
        self.editor.draw(self.started.elapsed());
        let (pw, ph) = self.editor.pixel_size();
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
            .zip(self.editor.pixels().as_chunks::<4>().0)
        {
            *out = [px[2], px[1], px[0], px[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(scale);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        buffer.attach_to(surface)?;
        self.window.commit();
        self.needs_draw = false;
        if let Some(shown) = &self.shown
            && let Ok(mut slot) = shown.lock()
        {
            let (pointer, drawing) = self.editor.pointer();
            *slot = Some(Shown {
                access: self.editor.access(),
                pointer,
                drawing,
            });
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
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete => Key::Delete,
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
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
            _ => self.size.or_else(|| {
                let (bw, bh) = configure.suggested_bounds.unwrap_or(SIZE);
                let fit = |want: u32, room: u32| {
                    if room == 0 {
                        want
                    } else {
                        want.min(room.saturating_sub(48))
                    }
                };
                Some((fit(SIZE.0, bw).max(640), fit(SIZE.1, bh).max(420)))
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

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

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
                    self.editor.pointer_move(at);
                    self.needs_draw = true;
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.editor.pointer_move(at);
                    self.closed |= self.editor.pointer_button(true);
                    self.needs_draw = true;
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.closed |= self.editor.pointer_button(false);
                    self.needs_draw = true;
                }
                _ => {}
            }
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
        // Ctrl and a letter: undo, copy, save.
        if self.modifiers.ctrl {
            if let Some(letter) = event.keysym.key_char().filter(char::is_ascii_alphabetic) {
                self.closed |= self.editor.shortcut(letter);
                self.needs_draw = true;
            }
            return;
        }
        match key_of(event.keysym) {
            Some(key) => self.closed |= self.editor.key(key),
            None => {
                if let Some(text) = event.utf8.as_deref()
                    && text.chars().all(|c| !c.is_control())
                {
                    self.editor.text(text);
                }
            }
        }
        self.needs_draw = true;
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
        modifiers: Modifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_never_writes_over_a_file() {
        let dir = tempfile::tempdir().expect("a directory");
        let capture = dir.path().join("Screenshot.png");
        let first = save_beside(&capture, b"a").expect("saved");
        let second = save_beside(&capture, b"b").expect("saved");
        assert!(first.ends_with("Screenshot (annotated).png"));
        assert!(second.ends_with("Screenshot (annotated 2).png"));
    }
}
