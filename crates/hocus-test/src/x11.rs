//! An X11 test client, for XWayland: one window filled with a solid colour,
//! which can own the clipboard and read it, and mark itself urgent.

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::Context;
use x11rb::{
    connection::Connection,
    properties::WmHints,
    protocol::{
        Event,
        xproto::{
            AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
            SelectionRequestEvent, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

/// A window of `width`×`height` pixels filled with `rgb`, its WM_CLASS set
/// to `app_id`.
#[derive(Debug, Clone)]
pub struct X11Window {
    pub app_id: String,
    pub width: u16,
    pub height: u16,
    pub rgb: u32,
}

#[derive(Debug)]
enum Command {
    Copy(String),
    Paste,
    /// Its urgency hint (WM_HINTS), set or taken back.
    Urgent(bool),
}

/// A running X11 test client.
#[derive(Debug)]
pub struct X11Client {
    pub spec: X11Window,
    commands: Sender<Command>,
    pasted: Arc<Mutex<Option<String>>>,
    thread: JoinHandle<anyhow::Result<()>>,
}

impl X11Client {
    /// Takes the clipboard with `text`.
    pub fn copy(&self, text: &str) -> anyhow::Result<()> {
        self.commands
            .send(Command::Copy(text.to_owned()))
            .context("the X11 client is gone")
    }

    /// Sets its urgency hint, or takes it back.
    pub fn urgent(&self, urgent: bool) -> anyhow::Result<()> {
        self.commands
            .send(Command::Urgent(urgent))
            .context("the X11 client is gone")
    }

    /// Asks for the clipboard; what arrives is in [`Self::pasted`].
    pub fn paste(&self) -> anyhow::Result<()> {
        self.commands
            .send(Command::Paste)
            .context("the X11 client is gone")
    }

    /// The text of the last paste that arrived.
    pub fn pasted(&self) -> Option<String> {
        self.pasted.lock().ok().and_then(|p| p.clone())
    }

    /// Whether the client is gone: closed, failed, or disconnected.
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
}

impl X11Window {
    /// Connects to X display `:display` and shows the window from a thread
    /// of its own, until the window manager closes it or goes away.
    pub fn spawn(self, display: u32) -> anyhow::Result<X11Client> {
        let (conn, screen) = x11rb::connect(Some(&format!(":{display}")))
            .with_context(|| format!("could not connect to X display :{display}"))?;
        let (commands, received) = channel();
        let pasted = Arc::new(Mutex::new(None));
        let thread = std::thread::Builder::new()
            .name(format!("x11 client {}", self.app_id))
            .spawn({
                let spec = self.clone();
                let pasted = pasted.clone();
                move || spec.run(conn, screen, received, pasted)
            })?;
        Ok(X11Client {
            spec: self,
            commands,
            pasted,
            thread,
        })
    }

    fn run(
        self,
        conn: RustConnection,
        screen: usize,
        commands: Receiver<Command>,
        pasted: Arc<Mutex<Option<String>>>,
    ) -> anyhow::Result<()> {
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)?;
        let window = conn.generate_id()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            self.width,
            self.height,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new()
                .background_pixel(self.rgb)
                .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
        )?;
        // WM_CLASS is the instance, then the class: the app id.
        let class = format!("{0}\0{0}\0", self.app_id);
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            class.as_bytes(),
        )?;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            self.app_id.as_bytes(),
        )?;
        // Closing goes through WM_DELETE_WINDOW, as for real applications.
        conn.change_property32(
            PropMode::REPLACE,
            window,
            atoms.wm_protocols,
            AtomEnum::ATOM,
            &[atoms.wm_delete_window],
        )?;
        conn.map_window(window)?;
        conn.flush()?;

        let mut copied: Option<String> = None;
        loop {
            while let Ok(command) = commands.try_recv() {
                match command {
                    Command::Copy(text) => {
                        conn.set_selection_owner(window, atoms.clipboard, x11rb::CURRENT_TIME)?;
                        copied = Some(text);
                    }
                    Command::Urgent(urgent) => {
                        let mut hints = WmHints::new();
                        hints.urgent = urgent;
                        hints.set(&conn, window)?;
                    }
                    Command::Paste => {
                        conn.convert_selection(
                            window,
                            atoms.clipboard,
                            atoms.utf8_string,
                            atoms.paste,
                            x11rb::CURRENT_TIME,
                        )?;
                    }
                }
                conn.flush()?;
            }
            let Some(event) = conn.poll_for_event()? else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            match event {
                Event::ClientMessage(message)
                    if message.data.as_data32()[0] == atoms.wm_delete_window =>
                {
                    conn.destroy_window(window)?;
                    conn.flush()?;
                    return Ok(());
                }
                Event::SelectionRequest(request) => {
                    answer(&conn, &atoms, &request, copied.as_deref())?;
                }
                Event::SelectionClear(_) => copied = None,
                Event::SelectionNotify(notify) if notify.property == atoms.paste => {
                    let reply = conn
                        .get_property(true, window, atoms.paste, AtomEnum::ANY, 0, u32::MAX / 4)?
                        .reply()?;
                    if let Ok(mut pasted) = pasted.lock() {
                        *pasted = Some(String::from_utf8_lossy(&reply.value).into_owned());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Gives a requestor the copied text, or the formats it comes in.
fn answer(
    conn: &RustConnection,
    atoms: &Atoms,
    request: &SelectionRequestEvent,
    copied: Option<&str>,
) -> anyhow::Result<()> {
    let property = match copied {
        Some(_) if request.target == atoms.targets => {
            conn.change_property32(
                PropMode::REPLACE,
                request.requestor,
                request.property,
                AtomEnum::ATOM,
                &[atoms.targets, atoms.utf8_string, atoms.text_plain],
            )?;
            request.property
        }
        Some(text) if request.target == atoms.utf8_string || request.target == atoms.text_plain => {
            conn.change_property8(
                PropMode::REPLACE,
                request.requestor,
                request.property,
                request.target,
                text.as_bytes(),
            )?;
            request.property
        }
        // Refused: no property.
        _ => x11rb::NONE,
    };
    let notify = SelectionNotifyEvent {
        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property,
    };
    conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
    conn.flush()?;
    Ok(())
}

struct Atoms {
    clipboard: u32,
    targets: u32,
    utf8_string: u32,
    text_plain: u32,
    paste: u32,
    wm_protocols: u32,
    wm_delete_window: u32,
}

impl Atoms {
    fn new(conn: &RustConnection) -> anyhow::Result<Self> {
        let atom = |name: &str| -> anyhow::Result<u32> {
            Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
        };
        Ok(Self {
            clipboard: atom("CLIPBOARD")?,
            targets: atom("TARGETS")?,
            utf8_string: atom("UTF8_STRING")?,
            text_plain: atom("text/plain;charset=utf-8")?,
            paste: atom("HOCUS_PASTE")?,
            wm_protocols: atom("WM_PROTOCOLS")?,
            wm_delete_window: atom("WM_DELETE_WINDOW")?,
        })
    }
}
