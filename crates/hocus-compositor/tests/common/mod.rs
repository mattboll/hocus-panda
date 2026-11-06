//! Test clients: real Wayland clients on threads of their own, connected
//! to an in-process headless compositor that the test drives. Each test
//! binary uses part of it.
#![allow(dead_code)]

use std::{os::unix::net::UnixStream, path::Path, sync::mpsc, time::Duration};

use anyhow::Context;
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};
use smithay_client_toolkit::{
    activation::{ActivationHandler, ActivationState, RequestData},
    compositor::{CompositorHandler, CompositorState},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
    },
    delegate_activation, delegate_compositor, delegate_data_device, delegate_layer,
    delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm,
    delegate_xdg_popup, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
        xdg::{
            XdgPositioner, XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{
        wl_callback, wl_data_device, wl_data_device_manager, wl_data_source, wl_keyboard,
        wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
    },
};

use wayland_protocols::{
    ext::idle_notify::v1::client::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::ExtIdleNotifierV1,
    },
    wp::{
        idle_inhibit::zv1::client::{
            zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1,
            zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
        },
        text_input::zv3::client::{
            zwp_text_input_manager_v3::ZwpTextInputManagerV3,
            zwp_text_input_v3::{self, ZwpTextInputV3},
        },
    },
    xdg::foreign::zv2::client::{
        zxdg_exported_v2::{self, ZxdgExportedV2},
        zxdg_exporter_v2::ZxdgExporterV2,
        zxdg_imported_v2::{self, ZxdgImportedV2},
        zxdg_importer_v2::ZxdgImporterV2,
    },
};
use wayland_protocols_misc::server_decoration::client::{
    org_kde_kwin_server_decoration::{self, OrgKdeKwinServerDecoration},
    org_kde_kwin_server_decoration_manager::OrgKdeKwinServerDecorationManager,
};

/// How long a test waits for what it checks before it fails. Only a failing
/// test waits it out, so it costs nothing when things go right. 10 s was
/// too short on CI's four shared CPUs: tests failed right at it while the
/// shell or the Settings were merely slow (#677, #679, #686, the featured
/// calendars' test).
pub const WAIT: Duration = Duration::from_secs(30);
/// What test windows are filled with, ARGB.
pub const WINDOW_COLOUR: u32 = 0xff3a_7bd5;
/// A client-side shadow: black at 25 %, premultiplied.
pub const SHADOW_COLOUR: u32 = 0x4000_0000;
pub const BTN_LEFT: u32 = 0x110;
pub const KEY_LEFTMETA: u32 = 125;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_UP: u32 = 103;
pub const KEY_LEFT: u32 = 105;
pub const KEY_RIGHT: u32 = 106;
pub const KEY_DOWN: u32 = 108;
pub const KEY_H: u32 = 35;
pub const KEY_1: u32 = 2;
pub const KEY_2: u32 = 3;
pub const KEY_3: u32 = 4;
pub const KEY_ESC: u32 = 1;
pub const KEY_TAB: u32 = 15;
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTALT: u32 = 56;
pub const KEY_Q: u32 = 16;
pub const KEY_F4: u32 = 62;

/// The shell's program, built next to the test's when the workspace is.
///
/// A test binary does not build it, so it may be older than its sources:
/// that fails the test rather than testing an old shell (#464).
pub fn shell_program() -> Option<std::path::PathBuf> {
    built("hocuspanda-shell", "hocus-shell")
}

/// The Settings' program (6.1), as [`shell_program`].
pub fn settings_program() -> Option<std::path::PathBuf> {
    built("hocus-settings", "hocus-settings")
}

/// The workspace's program `name` (of the package `package`), built next
/// to the test's, and fresh.
fn built(name: &str, package: &str) -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let program = exe.parent()?.parent()?.join(name);
    if !program.exists() {
        return None;
    }
    if let Err(why) = is_fresh(&program, name) {
        panic!("{why}: build it again (cargo build -p {package})");
    }
    Some(program)
}

/// Whether `program` was built from this checkout's sources as they are:
/// cargo's list of its sources (`NAME.d`, beside it) names only files of
/// this workspace, none of them newer than it.
fn is_fresh(program: &Path, name: &str) -> anyhow::Result<()> {
    let built = std::fs::metadata(program)?.modified()?;
    let list = program.with_extension("d");
    let text = std::fs::read_to_string(&list)
        .with_context(|| format!("{} is not there", list.display()))?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("no workspace")?;
    // `target: source source…`, spaces in names escaped.
    let (_, sources) = text.split_once(": ").context("an unreadable list")?;
    let sources = sources.trim().replace("\\ ", "\0");
    for source in sources.split_whitespace() {
        let source = std::path::PathBuf::from(source.replace('\0', " "));
        anyhow::ensure!(
            source.starts_with(workspace),
            "{name} was built from another checkout ({})",
            source.display()
        );
        let changed = std::fs::metadata(&source)
            .and_then(|m| m.modified())
            .with_context(|| format!("{name}'s source {} is gone", source.display()))?;
        anyhow::ensure!(
            changed <= built,
            "{name} is older than {}",
            source.display()
        );
    }
    Ok(())
}

/// A script in `dir` starting `shell` away from the machine's services:
/// no session bus, and a system bus, a PipeWire and backlights that are not
/// there, unless `env` (set after) names some.
pub fn shell_script(
    dir: &Path,
    shell: &Path,
    env: &[(&str, String)],
) -> anyhow::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let mut text = String::from(
        "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
         export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
         PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent\n",
    );
    for (name, value) in env {
        text.push_str(&format!("export {name}='{value}'\n"));
    }
    text.push_str(&format!("exec '{}' \"$@\"\n", shell.display()));
    let script = dir.join("shell");
    std::fs::write(&script, text)?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

/// How long a test waits for a private daemon (D-Bus, PipeWire) to take
/// connections: generous, as it only fails when the daemon never does.
pub const DAEMON_WAIT: Duration = Duration::from_secs(30);

/// A D-Bus daemon of the test's own, its socket `bus` in `dir` (its
/// configuration beside it), and its address, once it takes connections.
///
/// Its socket's file comes as the daemon binds it, a moment before it
/// listens: a connection then is refused. It prints its address
/// (`--print-address`) once it listens, which this waits for.
pub fn private_bus(dir: &Path) -> anyhow::Result<(std::process::Child, String)> {
    use std::{
        io::BufRead,
        process::{Command, Stdio},
    };
    let socket = dir.join("bus");
    let config = dir.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:path={}</listen>\
             <policy context=\"default\"><allow send_destination=\"*\"/>\
             <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
            socket.display()
        ),
    )?;
    let mut bus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address=1"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("dbus-daemon")?;
    let out = bus.stdout.take().context("dbus-daemon's output")?;
    // Read off the test's thread: a daemon that hangs fails the test
    // after a while rather than holding it.
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let read = std::io::BufReader::new(out).read_line(&mut line);
        let _ = said.send(read.map(|_| line));
    });
    let address = match heard.recv_timeout(DAEMON_WAIT) {
        Ok(Ok(line)) if !line.trim().is_empty() => line.trim().to_owned(),
        Ok(read) => {
            let _ = bus.kill();
            let _ = bus.wait();
            anyhow::bail!("dbus-daemon ended without listening: {read:?}");
        }
        Err(_) => {
            let _ = bus.kill();
            let _ = bus.wait();
            anyhow::bail!("dbus-daemon did not listen within {DAEMON_WAIT:?}");
        }
    };
    anyhow::ensure!(
        address.contains(&socket.display().to_string()),
        "dbus-daemon listens at {address}, not {}",
        socket.display()
    );
    Ok((bus, format!("unix:path={}", socket.display())))
}

/// Waits until the Unix socket at `path` takes connections, as a daemon's
/// does once it listens: its file alone comes a moment before (bound, not
/// listening yet), and a connection then is refused.
pub fn wait_accepts(path: &Path) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    loop {
        match UnixStream::connect(path) {
            Ok(_) => return Ok(()),
            Err(err) => anyhow::ensure!(
                start.elapsed() < DAEMON_WAIT,
                "{} never took a connection: {err}",
                path.display()
            ),
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The render node for the tests that need a GPU (the shell's motion on
/// wgpu, timings): the one `HOCUS_TEST_GPU` names, never one found by
/// itself, which may be the GPU of the session the tests run in (#534);
/// `None`, and the test is skipped or runs on the CPU alone.
pub fn render_node() -> Option<std::path::PathBuf> {
    let node = hocus_compositor::test_gpu::node();
    if node.is_none() {
        eprintln!(
            "no GPU: {} names no render node (HOCUS_TEST_GPU=/dev/dri/renderD128 to draw on one)",
            hocus_compositor::test_gpu::ENV
        );
    }
    node
}

/// A compositor drawing with GLES, for the tests of its shaders (the
/// streak, the halo, the glass, the rim's ring): on the GPU
/// `HOCUS_TEST_GPU` names, else on Mesa's software device; with pixman
/// without that, which those tests see in `renderer()` and skip.
pub fn gles() -> HeadlessConfig {
    let config = hocus_compositor::test_gpu::gles();
    match (config.renderer, &config.render_node) {
        (_, Some(node)) => eprintln!("GLES on the GPU {}", node.display()),
        (RendererChoice::GlesSoftware, _) => eprintln!("GLES on Mesa's software device"),
        _ => eprintln!("no GLES here, not even Mesa's software device: pixman"),
    }
    config
}

pub fn compositor() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        // The same pixels on every machine, and no GPU needed.
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

/// Two screens side by side: 1920×1080, then 2560×1440 on its right.
pub fn two_screens() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

/// What a test client shows.
#[derive(Debug, Clone)]
pub enum Shows {
    /// An xdg toplevel of this size, with this app id; with `popup`, a
    /// press on it opens a menu that grabs the keyboard.
    Window {
        app_id: &'static str,
        size: (u32, u32),
        popup: bool,
    },
    /// An xdg toplevel of this size that asks to be fullscreen, or
    /// maximised, before its first commit, as `foot --fullscreen` does.
    Asking {
        app_id: &'static str,
        size: (u32, u32),
        fullscreen: bool,
    },
    /// A layer surface along the output's top edge, `height` high, which
    /// reserves its height.
    Bar { height: u32 },
    /// A clear layer surface in the bar's namespace, where the shell puts
    /// its full bar: what shows there is the compositor's glass alone.
    ClearBar,
    /// A layer surface of the top layer in `namespace`, filled with
    /// `colour` (ARGB), `size` large, anchored to the screen's top right
    /// corner `margin` (top, right) from it; a height of 0 spans the
    /// screen's height less the top margin at both ends, as the bar does.
    /// It reserves nothing.
    Layer {
        namespace: &'static str,
        size: (u32, u32),
        margin: (i32, i32),
        colour: u32,
    },
    /// An xdg toplevel of this size drawn with a translucent shadow
    /// `margin` wide around it, outside its window geometry, as
    /// client-side decorated applications draw theirs.
    Shadowed {
        app_id: &'static str,
        size: (u32, u32),
        margin: u32,
    },
    /// An xdg toplevel of this size that, floating, takes the sizes asked
    /// rounded down to whole `cell`s, as a terminal does (foot's
    /// `resize-by-cells`); tiled, maximised or fullscreen, the sizes asked.
    Cells {
        app_id: &'static str,
        size: (u32, u32),
        cell: u32,
    },
    /// An xdg toplevel of this size drawn in black and white squares of
    /// 3 pixels, as fine as small text: what a blur makes unreadable.
    Checkered {
        app_id: &'static str,
        size: (u32, u32),
    },
    /// An xdg toplevel of this size that takes the keyboard and says the
    /// layout (XKB's group) each modifiers event gives it.
    Typing {
        app_id: &'static str,
        size: (u32, u32),
    },
    /// A typing window with a text field: with `caret`, through
    /// text-input-v3, enabled as it takes the keyboard (and disabled as
    /// it loses it), its caret there (surface-local); it says what is
    /// committed to it ([`Event::Committed`]). With or without, `Ctrl+V`
    /// reads the clipboard's text ([`Event::Pasted`]).
    Writing {
        app_id: &'static str,
        size: (u32, u32),
        caret: Option<(i32, i32, i32, i32)>,
    },
    /// An xdg toplevel of this size that hides the pointer over it at each
    /// enter (`wl_pointer.set_cursor` without a surface), as a video
    /// player does: a shape of its own, which the next surface's client
    /// must replace.
    Hiding {
        app_id: &'static str,
        size: (u32, u32),
    },
    /// An xdg toplevel of this size that says what its pointer gets:
    /// [`Event::Pointer`].
    Pointing {
        app_id: &'static str,
        size: (u32, u32),
    },
    /// An xdg toplevel of this size without xdg-decoration that asks for
    /// the server's decorations (`server`) or draws its own through KDE's
    /// server decoration protocol, as GTK 3 clients do; it says
    /// the modes the compositor gives it ([`Event::KdeMode`]).
    KdeDecorated {
        app_id: &'static str,
        size: (u32, u32),
        server: bool,
    },
    /// An xdg toplevel of this size that asks for nothing through either
    /// decoration protocol, as GTK 4 applications, which draw their own
    /// header.
    Undecorated {
        app_id: &'static str,
        size: (u32, u32),
    },
}

/// What the test asks of a client.
#[derive(Debug, Clone)]
pub enum Command {
    Fullscreen(bool),
    /// Takes the clipboard (it must hold the keyboard), then says
    /// [`Event::Copied`] once the compositor has it.
    Copy(Clip),
    /// Takes the clipboard with a PNG image (`image/png`), as [`Command::Copy`].
    CopyPng(&'static [u8]),
    /// Closes its window, as an application quitting.
    Close,
    /// Titles its window, without drawing (a terminal's prompt does so).
    Title(&'static str),
    /// Reads the clipboard's text, as `Ctrl+V` does: [`Event::Pasted`].
    ReadClipboard,
    /// Asks for its window to be activated (`xdg_activation_v1`) with this
    /// token; without one, with a token it asks for itself, without the
    /// user's input, as an application does that was activated over D-Bus.
    Activate(Option<String>),
    /// Opens a window, as an application hidden in the tray shows one.
    Open {
        app_id: &'static str,
        size: (u32, u32),
    },
    /// Exports its window through xdg-foreign: [`Event::Exported`].
    Export,
    /// Opens a window made, before its first frame, the child of the one
    /// xdg-foreign's `handle` names, as the desktop portal's backend opens
    /// its dialogs; [`Event::ImportGone`] if the handle names nothing.
    OpenDialog {
        app_id: &'static str,
        size: (u32, u32),
        handle: String,
    },
    /// Holds idleness off with an idle inhibitor on its window's (or
    /// layer's) surface, as a video player does; `false` destroys it.
    Inhibit(bool),
    /// Asks to be told when the seat is idle this long
    /// (`ext_idle_notifier_v1`); with `input`, input alone counts
    /// (`get_input_idle_notification`): [`Event::Idled`], [`Event::Resumed`].
    IdleNotification {
        timeout_ms: u32,
        input: bool,
    },
}

/// What a client puts on the clipboard: its types, and the text it writes
/// for any of them (`secret` for `x-kde-passwordManagerHint`, as a password
/// manager marks a password); or, `hang`ing, the pipe kept open and nothing
/// written, as a client that stopped answering.
#[derive(Debug, Clone, Copy)]
pub struct Clip {
    pub types: &'static [&'static str],
    pub text: &'static str,
    pub hang: bool,
}

/// What a client tells the test.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Globals(Vec<String>),
    PopupShown,
    PopupDone,
    /// The compositor asked its window to close; it stays.
    CloseRequested,
    /// The compositor has its clipboard.
    Copied,
    /// The keyboard's modifiers came, with this layout (XKB's group).
    Group(u32),
    /// A typing client's key (its evdev code), pressed or released.
    Key(u32, bool),
    /// A writing client's text input got this text (`commit_string`, then
    /// `done`).
    Committed(String),
    /// A writing client read this text from the clipboard (empty: none).
    Pasted(String),
    /// A pointing client's pointer got this.
    Pointer(Pointed),
    /// Its idle notification (made with `input` or not) became idle.
    Idled {
        input: bool,
    },
    /// It is no longer idle.
    Resumed {
        input: bool,
    },
    /// Its window's xdg-foreign handle.
    Exported(String),
    /// The handle it imported names nothing (any more).
    ImportGone,
    /// Its KDE server decoration's mode came: the server's, or not.
    KdeMode {
        server: bool,
    },
}

/// What a pointing client's pointer got ([`Shows::Pointing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointed {
    Enter,
    Leave,
    Motion,
    Press,
    Release,
}

/// A client running on its thread.
#[derive(Debug)]
pub struct ClientHandle {
    pub events: mpsc::Receiver<Event>,
    pub commands: mpsc::Sender<Command>,
}

/// Starts a client on a thread of its own. It runs until the compositor
/// goes.
pub fn client(socket: &Path, shows: Option<Shows>) -> anyhow::Result<ClientHandle> {
    let stream = UnixStream::connect(socket)
        .with_context(|| format!("could not connect to {}", socket.display()))?;
    stream.set_nonblocking(true)?;
    let conn = Connection::from_socket(stream)?;
    let (events, events_rx) = mpsc::channel();
    let (commands_tx, commands) = mpsc::channel();
    std::thread::Builder::new()
        .name("test client".into())
        .spawn(move || {
            if let Err(err) = run_client(&conn, shows, &events, &commands) {
                // The compositor going ends every client.
                eprintln!("test client: {err:#}");
            }
        })?;
    Ok(ClientHandle {
        events: events_rx,
        commands: commands_tx,
    })
}

fn run_client(
    conn: &Connection,
    shows: Option<Shows>,
    events: &mpsc::Sender<Event>,
    commands: &mpsc::Receiver<Command>,
) -> anyhow::Result<()> {
    let (globals, mut queue) = registry_queue_init(conn)?;
    let qh = queue.handle();
    let names = globals.contents().with_list(|list| {
        list.iter()
            .map(|g| format!("{} v{}", g.interface, g.version))
            .collect()
    });
    events.send(Event::Globals(names))?;
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let xdg = XdgShell::bind(&globals, &qh).ok();
    let data = DataDeviceManagerState::bind(&globals, &qh).ok();
    let mut client = Client {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        pool: SlotPool::new(512 * 512 * 4, &shm)?,
        shm,
        size: (1, 1),
        window: None,
        layer: None,
        popup: None,
        popup_on_press: false,
        hiding: false,
        pointing: false,
        margin: 0,
        cell: 0,
        seat: None,
        events: events.clone(),
        compositor,
        xdg,
        data,
        device: None,
        clip: None,
        png: None,
        hung: Vec::new(),
        typing: false,
        clear: false,
        checkered: false,
        fill: None,
        caret: None,
        text_inputs: None,
        text_input: None,
        committing: None,
        ctrl: false,
        writing: false,
        activation: ActivationState::bind(&globals, &qh).ok(),
        inhibitor: None,
        notifications: Vec::new(),
        exported: None,
        imported: None,
        kde_decoration: None,
    };
    match shows {
        Some(Shows::Window {
            app_id,
            size,
            popup,
        }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.popup_on_press = popup;
        }
        Some(Shows::Shadowed {
            app_id,
            size,
            margin,
        }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestClient, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.margin = margin;
            client.window = Some(window);
        }
        Some(Shows::Hiding { app_id, size }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.hiding = true;
        }
        Some(Shows::Pointing { app_id, size }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.pointing = true;
        }
        Some(Shows::Checkered { app_id, size }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.checkered = true;
        }
        Some(Shows::Typing { app_id, size }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.typing = true;
        }
        Some(Shows::Writing {
            app_id,
            size,
            caret,
        }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.typing = true;
            client.writing = true;
            client.caret = caret;
            if caret.is_some() {
                client.text_inputs = Some(
                    globals
                        .bind::<ZwpTextInputManagerV3, _, _>(&qh, 1..=1, ())
                        .context("no text input")?,
                );
            }
        }
        Some(Shows::KdeDecorated {
            app_id,
            size,
            server,
        }) => {
            use org_kde_kwin_server_decoration::Mode;
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let manager = globals
                .bind::<OrgKdeKwinServerDecorationManager, _, _>(&qh, 1..=1, ())
                .context("no KDE server decoration")?;
            let surface = client.compositor.create_surface(&qh);
            let decoration = manager.create(&surface, &qh, ());
            decoration.request_mode(if server { Mode::Server } else { Mode::Client });
            let window = xdg.create_window(surface, WindowDecorations::None, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.kde_decoration = Some(decoration);
        }
        Some(Shows::Undecorated { app_id, size }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::None, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
        }
        Some(Shows::Cells { app_id, size, cell }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.cell = cell;
            client.window = Some(window);
        }
        Some(Shows::Asking {
            app_id,
            size,
            fullscreen,
        }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            if fullscreen {
                window.set_fullscreen(None);
            } else {
                window.set_maximized();
            }
            window.commit();
            client.size = size;
            client.window = Some(window);
        }
        Some(Shows::ClearBar) => {
            let shell = LayerShell::bind(&globals, &qh).context("no layer shell")?;
            let surface = client.compositor.create_surface(&qh);
            let layer = shell.create_layer_surface(
                &qh,
                surface,
                Layer::Top,
                Some(hocus_link::NAMESPACE),
                None,
            );
            let edge = hocus_link::EDGE;
            layer.set_anchor(Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM);
            layer.set_size(hocus_link::Form::Full.width() as u32, 0);
            layer.set_margin(edge, edge, edge, 0);
            layer.commit();
            client.clear = true;
            client.layer = Some(layer);
        }
        Some(Shows::Layer {
            namespace,
            size,
            margin: (top, right),
            colour,
        }) => {
            let shell = LayerShell::bind(&globals, &qh).context("no layer shell")?;
            let surface = client.compositor.create_surface(&qh);
            let layer = shell.create_layer_surface(&qh, surface, Layer::Top, Some(namespace), None);
            let mut anchor = Anchor::RIGHT | Anchor::TOP;
            if size.1 == 0 {
                anchor |= Anchor::BOTTOM;
            }
            layer.set_anchor(anchor);
            layer.set_size(size.0, size.1);
            layer.set_margin(top, right, top, 0);
            layer.commit();
            client.fill = Some(colour);
            client.layer = Some(layer);
        }
        Some(Shows::Bar { height }) => {
            let shell = LayerShell::bind(&globals, &qh).context("no layer shell")?;
            let surface = client.compositor.create_surface(&qh);
            let layer = shell.create_layer_surface(&qh, surface, Layer::Top, Some("bar"), None);
            layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, height);
            layer.set_exclusive_zone(height as i32);
            layer.commit();
            client.size = (1, height);
            client.layer = Some(layer);
        }
        None => {}
    }
    loop {
        queue.dispatch_pending(&mut client)?;
        while let Ok(command) = commands.try_recv() {
            match (command, &client.window) {
                (Command::Fullscreen(true), Some(window)) => window.set_fullscreen(None),
                (Command::Fullscreen(false), Some(window)) => window.unset_fullscreen(),
                (Command::Fullscreen(_), None) => {}
                (Command::Copy(clip), _) => {
                    client.png = None;
                    client.copy(conn, &qh, clip);
                }
                (Command::CopyPng(png), _) => {
                    client.png = Some(png);
                    let clip = Clip {
                        types: &["image/png"],
                        text: "",
                        hang: false,
                    };
                    client.copy(conn, &qh, clip);
                }
                (Command::Close, _) => client.window = None,
                (Command::Title(title), Some(window)) => window.set_title(title),
                (Command::Title(_), None) => {}
                (Command::ReadClipboard, _) => client.read_clipboard(),
                (Command::Activate(token), Some(window)) => {
                    let surface = window.wl_surface().clone();
                    match (&client.activation, token) {
                        (Some(activation), Some(token)) => {
                            activation.activate::<Client>(&surface, token);
                        }
                        (Some(activation), None) => activation.request_token(
                            &qh,
                            RequestData {
                                app_id: None,
                                seat_and_serial: None,
                                surface: Some(surface),
                            },
                        ),
                        (None, _) => {}
                    }
                }
                (Command::Activate(_), None) => {}
                (Command::Inhibit(true), _) => {
                    let surface = match (&client.window, &client.layer) {
                        (Some(window), _) => Some(window.wl_surface().clone()),
                        (None, Some(layer)) => Some(layer.wl_surface().clone()),
                        (None, None) => None,
                    };
                    let manager = globals.bind::<ZwpIdleInhibitManagerV1, _, _>(&qh, 1..=1, ());
                    if let (Some(surface), Ok(manager)) = (surface, manager) {
                        client.inhibitor = Some(manager.create_inhibitor(&surface, &qh, ()));
                    }
                }
                (Command::Inhibit(false), _) => {
                    if let Some(inhibitor) = client.inhibitor.take() {
                        inhibitor.destroy();
                    }
                }
                (Command::IdleNotification { timeout_ms, input }, _) => {
                    let notifier = globals.bind::<ExtIdleNotifierV1, _, _>(&qh, 2..=2, ());
                    if let (Ok(notifier), Some(seat)) = (notifier, client.seats.seats().next()) {
                        let notification = if input {
                            notifier.get_input_idle_notification(timeout_ms, &seat, &qh, input)
                        } else {
                            notifier.get_idle_notification(timeout_ms, &seat, &qh, input)
                        };
                        client.notifications.push(notification);
                    }
                }
                (Command::Export, Some(window)) => {
                    if let Ok(exporter) = globals.bind::<ZxdgExporterV2, _, _>(&qh, 1..=1, ()) {
                        client.exported =
                            Some(exporter.export_toplevel(window.wl_surface(), &qh, ()));
                    }
                }
                (Command::Export, None) => {}
                (
                    Command::OpenDialog {
                        app_id,
                        size,
                        handle,
                    },
                    _,
                ) => {
                    let importer = globals.bind::<ZxdgImporterV2, _, _>(&qh, 1..=1, ());
                    if let (Some(xdg), Ok(importer)) = (&client.xdg, importer) {
                        let surface = client.compositor.create_surface(&qh);
                        let window =
                            xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
                        window.set_app_id(app_id);
                        let imported = importer.import_toplevel(handle, &qh, ());
                        imported.set_parent_of(window.wl_surface());
                        window.commit();
                        client.size = size;
                        client.window = Some(window);
                        client.imported = Some(imported);
                    }
                }
                (Command::Open { app_id, size }, _) => {
                    if let Some(xdg) = &client.xdg {
                        let surface = client.compositor.create_surface(&qh);
                        let window =
                            xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
                        window.set_app_id(app_id);
                        window.commit();
                        client.size = size;
                        client.window = Some(window);
                    }
                }
            }
        }
        queue.flush()?;
        if let Some(guard) = queue.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(err))
                    if err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}

struct Client {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    pool: SlotPool,
    size: (u32, u32),
    window: Option<Window>,
    layer: Option<LayerSurface>,
    popup: Option<Popup>,
    popup_on_press: bool,
    /// Hides the pointer over its window ([`Shows::Hiding`]).
    hiding: bool,
    /// Says what its pointer gets ([`Shows::Pointing`]).
    pointing: bool,
    /// The shadow around the window, outside its geometry.
    margin: u32,
    /// Floating, its sizes are whole multiples of this.
    cell: u32,
    seat: Option<wl_seat::WlSeat>,
    events: mpsc::Sender<Event>,
    compositor: CompositorState,
    xdg: Option<XdgShell>,
    data: Option<DataDeviceManagerState>,
    device: Option<DataDevice>,
    /// What it offers on the clipboard, and its source; an image instead
    /// of its text.
    clip: Option<(Clip, CopyPasteSource)>,
    png: Option<&'static [u8]>,
    /// The pipes it was asked to write to and never will.
    hung: Vec<WritePipe>,
    /// It takes the keyboard and says its group.
    typing: bool,
    /// Its layer surface is clear.
    clear: bool,
    /// Drawn in black and white squares.
    checkered: bool,
    /// Its surface's colour, when not a window's.
    fill: Option<u32>,
    /// A writing client's caret, its text input, and what is committed
    /// to it until `done`; whether Ctrl is held.
    caret: Option<(i32, i32, i32, i32)>,
    text_inputs: Option<ZwpTextInputManagerV3>,
    text_input: Option<ZwpTextInputV3>,
    committing: Option<String>,
    ctrl: bool,
    writing: bool,
    activation: Option<ActivationState>,
    inhibitor: Option<ZwpIdleInhibitorV1>,
    notifications: Vec<ExtIdleNotificationV1>,
    exported: Option<ZxdgExportedV2>,
    imported: Option<ZxdgImportedV2>,
    kde_decoration: Option<OrgKdeKwinServerDecoration>,
}

impl Client {
    fn copy(&mut self, conn: &Connection, qh: &QueueHandle<Self>, clip: Clip) {
        let (Some(data), Some(seat)) = (&self.data, self.seats.seats().next()) else {
            return;
        };
        let device = self
            .device
            .get_or_insert_with(|| data.get_data_device(qh, &seat));
        let source = data.create_copy_paste_source(qh, clip.types.iter().copied());
        // The compositor checks the keyboard, not the serial.
        source.set_selection(device, 0);
        self.clip = Some((clip, source));
        conn.display().sync(qh, ());
    }

    /// Reads the clipboard's text off its thread: [`Event::Pasted`].
    fn read_clipboard(&mut self) {
        let offer = self
            .device
            .as_ref()
            .and_then(|d| d.data().selection_offer());
        let events = self.events.clone();
        let Some(pipe) = offer.and_then(|o| o.receive("text/plain;charset=utf-8".to_owned()).ok())
        else {
            let _ = events.send(Event::Pasted(String::new()));
            return;
        };
        std::thread::spawn(move || {
            use std::io::Read;
            let mut file = std::fs::File::from(std::os::fd::OwnedFd::from(pipe));
            let mut text = String::new();
            let _ = file.read_to_string(&mut text);
            let _ = events.send(Event::Pasted(text));
        });
    }

    /// Fills `surface` with a solid colour, `size` large.
    fn draw(&mut self, surface: &wl_surface::WlSurface, size: (u32, u32)) {
        self.draw_framed(surface, size, 0);
    }

    /// A solid colour `size` large, within a translucent shadow `margin`
    /// wide.
    fn draw_framed(&mut self, surface: &wl_surface::WlSurface, size: (u32, u32), margin: u32) {
        let m = margin as i32;
        let (iw, ih) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let (w, h) = (iw + 2 * m, ih + 2 * m);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        for (i, pixel) in canvas.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (i as i32 % w, i as i32 / w);
            let inside = x >= m && y >= m && x < m + iw && y < m + ih;
            let colour: u32 = match (self.clear, inside) {
                (true, _) => 0,
                (false, true) if self.checkered => {
                    if ((x - m) / 3 + (y - m) / 3) % 2 == 0 {
                        0xffff_ffff
                    } else {
                        0xff00_0000
                    }
                }
                (false, true) => self.fill.unwrap_or(WINDOW_COLOUR),
                (false, false) => SHADOW_COLOUR,
            };
            pixel.copy_from_slice(&colour.to_le_bytes());
        }
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            surface.commit();
        }
    }

    /// A menu under the press, grabbing the keyboard.
    fn open_popup(&mut self, qh: &QueueHandle<Self>, serial: u32) {
        let (Some(window), Some(xdg), Some(seat)) = (&self.window, &self.xdg, &self.seat) else {
            return;
        };
        let Ok(positioner) = XdgPositioner::new(xdg) else {
            return;
        };
        positioner.set_size(100, 80);
        positioner.set_anchor_rect(10, 10, 1, 1);
        let Ok(popup) = Popup::new(window.xdg_surface(), &positioner, qh, &self.compositor, xdg)
        else {
            return;
        };
        popup.xdg_popup().grab(seat, serial);
        popup.wl_surface().commit();
        self.popup = Some(popup);
    }
}

impl WindowHandler for Client {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        let _ = self.events.send(Event::CloseRequested);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        if let (Some(w), Some(h)) = configure.new_size {
            self.size = (w.get(), h.get());
            let floating = !(configure.is_maximized()
                || configure.is_fullscreen()
                || configure.is_tiled_left()
                || configure.is_tiled_right()
                || configure.is_tiled_top()
                || configure.is_tiled_bottom());
            if self.cell > 0 && floating {
                let c = self.cell;
                self.size = ((self.size.0 / c).max(1) * c, (self.size.1 / c).max(1) * c);
            }
        }
        let surface = window.wl_surface().clone();
        let m = self.margin as i32;
        if m > 0 {
            window
                .xdg_surface()
                .set_window_geometry(m, m, self.size.0 as i32, self.size.1 as i32);
        }
        self.draw_framed(&surface, self.size, self.margin);
    }
}

impl PopupHandler for Client {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        let surface = popup.wl_surface().clone();
        let size = (config.width.max(1) as u32, config.height.max(1) as u32);
        self.draw(&surface, size);
        let _ = self.events.send(Event::PopupShown);
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Popup) {
        self.popup = None;
        let _ = self.events.send(Event::PopupDone);
    }
}

impl LayerShellHandler for Client {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {}

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.size = (configure.new_size.0.max(1), configure.new_size.1.max(1));
        let surface = layer.wl_surface().clone();
        self.draw(&surface, self.size);
    }
}

impl SeatHandler for Client {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && (self.popup_on_press || self.hiding || self.pointing)
        {
            let _ = self.seats.get_pointer(qh, &seat);
            self.seat = Some(seat);
        } else if capability == Capability::Keyboard && self.typing {
            seat.get_keyboard(qh, ());
            // To read the clipboard on Ctrl+V.
            if self.writing
                && let Some(data) = &self.data
                && self.device.is_none()
            {
                self.device = Some(data.get_data_device(qh, &seat));
            }
            if let Some(manager) = &self.text_inputs {
                self.text_input = Some(manager.get_text_input(&seat, qh, ()));
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Client {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if self.pointing {
                let pointed = match event.kind {
                    PointerEventKind::Enter { .. } => Some(Pointed::Enter),
                    PointerEventKind::Leave { .. } => Some(Pointed::Leave),
                    PointerEventKind::Motion { .. } => Some(Pointed::Motion),
                    PointerEventKind::Press { .. } => Some(Pointed::Press),
                    PointerEventKind::Release { .. } => Some(Pointed::Release),
                    _ => None,
                };
                if let Some(pointed) = pointed {
                    let _ = self.events.send(Event::Pointer(pointed));
                }
            }
            match event.kind {
                PointerEventKind::Enter { serial } if self.hiding => {
                    pointer.set_cursor(serial, None, 0, 0);
                }
                PointerEventKind::Press { serial, .. }
                    if self.popup_on_press && self.popup.is_none() =>
                {
                    self.open_popup(qh, serial);
                }
                _ => {}
            }
        }
    }
}

impl CompositorHandler for Client {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
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

impl OutputHandler for Client {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Client {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Client);
delegate_output!(Client);
delegate_shm!(Client);
delegate_seat!(Client);
delegate_pointer!(Client);
delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);
delegate_xdg_popup!(Client);
delegate_layer!(Client);
delegate_registry!(Client);
delegate_data_device!(Client);
delegate_activation!(Client);

impl ActivationHandler for Client {
    type RequestData = RequestData;

    /// The token it asked for: its window asks for the focus with it.
    fn new_token(&mut self, token: String, data: &RequestData) {
        if let (Some(activation), Some(surface)) = (&self.activation, &data.surface) {
            activation.activate::<Client>(surface, token);
        }
    }
}

/// A typing client's keyboard: its modifiers' group.
impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Modifiers {
                group,
                mods_depressed,
                ..
            } => {
                // XKB's Control.
                client.ctrl = mods_depressed & 0x4 != 0;
                let _ = client.events.send(Event::Group(group));
            }
            wl_keyboard::Event::Key { key, state, .. } => {
                let pressed = state == wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed);
                let _ = client.events.send(Event::Key(key, pressed));
                // Ctrl+V pastes in a writing window.
                if pressed && client.ctrl && key == 47 && client.device.is_some() {
                    client.read_clipboard();
                }
            }
            _ => {}
        }
    }
}

/// The answer to the sync after a copy.
impl Dispatch<wl_callback::WlCallback, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let _ = client.events.send(Event::Copied);
    }
}

impl DataSourceHandler for Client {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        mime: String,
        mut pipe: WritePipe,
    ) {
        use std::io::Write;
        let Some((clip, _)) = &self.clip else {
            return;
        };
        if clip.hang {
            self.hung.push(pipe);
        } else if mime == "x-kde-passwordManagerHint" {
            let _ = pipe.write_all(b"secret");
        } else if let Some(png) = self.png {
            let _ = pipe.write_all(png);
        } else {
            let _ = pipe.write_all(clip.text.as_bytes());
        }
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: wl_data_device_manager::DndAction,
    ) {
    }
}

impl DataDeviceHandler for Client {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
    ) {
    }

    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }

    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
}

impl DataOfferHandler for Client {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: wl_data_device_manager::DndAction,
    ) {
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: wl_data_device_manager::DndAction,
    ) {
    }
}

/// Opens a window and waits until the compositor has mapped it.
pub fn open_with(
    hp: &mut Headless,
    app_id: &'static str,
    size: (u32, u32),
    popup: bool,
) -> anyhow::Result<(Rect, ClientHandle)> {
    open_showing(
        hp,
        app_id,
        Shows::Window {
            app_id,
            size,
            popup,
        },
    )
}

/// Starts a client showing `shows`, a window with `app_id`, and waits until
/// the compositor has mapped it.
pub fn open_showing(
    hp: &mut Headless,
    app_id: &'static str,
    shows: Shows,
) -> anyhow::Result<(Rect, ClientHandle)> {
    let before = hp.windows().len();
    let handle = client(hp.socket(), Some(shows))?;
    let mapped = hp.dispatch_until(WAIT, |hp| hp.windows().len() > before)?;
    anyhow::ensure!(mapped, "{app_id} was never mapped");
    let rect = rect_of(hp, app_id).context("no such window")?;
    Ok((rect, handle))
}

pub fn open(hp: &mut Headless, app_id: &'static str, size: (u32, u32)) -> anyhow::Result<Rect> {
    let (rect, handle) = open_with(hp, app_id, size, false)?;
    // The client runs as long as the compositor does.
    std::mem::forget(handle);
    Ok(rect)
}

/// A mapped window's geometry.
pub fn rect_of(hp: &Headless, app_id: &str) -> Option<Rect> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.rect)
}

/// Runs the compositor until `app_id`'s geometry is `rect`.
pub fn wait_for_rect(hp: &mut Headless, app_id: &str, rect: Rect) -> anyhow::Result<()> {
    let reached = hp.dispatch_until(WAIT, |hp| rect_of(hp, app_id) == Some(rect))?;
    anyhow::ensure!(
        reached,
        "{app_id} at {:?}, not {rect:?}",
        rect_of(hp, app_id)
    );
    Ok(())
}

/// Presses the keys in order, then releases them in reverse.
pub fn chord(hp: &mut Headless, keys: &[u32]) {
    for key in keys {
        hp.key(*key, true);
    }
    for key in keys.iter().rev() {
        hp.key(*key, false);
    }
}

/// Runs the compositor for `time` of real time.
pub fn wait(hp: &mut Headless, time: Duration) -> anyhow::Result<()> {
    hp.dispatch_until(time, |_| false)?;
    Ok(())
}

/// The pointer at `at`, once the shell's surface of `namespace` takes it
/// there. A layer surface maps (and has its place) before the shell has
/// drawn it, and until it has a buffer it takes no pointer: a press then
/// goes to what is under it and is lost. On a loaded machine, with the
/// shell drawing on the CPU, that is long enough for a test's click.
#[allow(dead_code)]
pub fn over(hp: &mut Headless, at: (f64, f64), namespace: &str) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    loop {
        hp.pointer_move(at.0 - 1.0, at.1);
        hp.pointer_move(at.0, at.1);
        wait(hp, Duration::from_millis(30))?;
        // Drawn at its size too: a surface resized takes the pointer with
        // its old buffer, its parts not where they will be.
        if hp.pointer_layer().as_deref() == Some(namespace) && hp.undrawn_at(at.0, at.1).is_empty()
        {
            return Ok(());
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(15),
            "{namespace} never took the pointer at {at:?}: {:?}",
            hp.pointer_layer()
        );
    }
}

/// The pointer moved to `at` (twice: the first move may only enter a
/// surface), once every shell surface placed there shows what was asked of
/// it ([`Headless::undrawn_at`]): a layer surface has its place before its
/// buffer, and on a loaded machine the shell draws it long after.
pub fn point(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<()> {
    let drawn = hp.dispatch_until(WAIT, |hp| hp.undrawn_at(x, y).is_empty())?;
    anyhow::ensure!(
        drawn,
        "at {:?}, never drawn: {:?}",
        (x, y),
        hp.undrawn_at(x, y)
    );
    hp.pointer_move(x - 1.0, y);
    hp.pointer_move(x, y);
    Ok(())
}

/// A click at `at` ([`point`]).
pub fn click(hp: &mut Headless, at: (f64, f64)) -> anyhow::Result<()> {
    point(hp, at)?;
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    Ok(())
}

/// A click at `at` on the shell's surface of `namespace`, once it takes
/// the pointer there ([`over`]).
pub fn click_on(hp: &mut Headless, at: (f64, f64), namespace: &str) -> anyhow::Result<()> {
    over(hp, at, namespace)?;
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    Ok(())
}

/// The shell's surfaces of `namespace`, once one at least shows what was
/// asked of it ([`Headless::drawn_layers`]): where they are.
pub fn drawn(hp: &mut Headless, namespace: &str) -> anyhow::Result<Vec<(String, Rect)>> {
    let done = hp.dispatch_until(WAIT, |hp| !hp.drawn_layers(namespace).is_empty())?;
    anyhow::ensure!(
        done,
        "{namespace} never drawn: placed {:?}",
        hp.layer_rects(namespace)
    );
    Ok(hp.drawn_layers(namespace))
}

/// Where the shell's popovers are, those drawn there only
/// ([`Headless::drawn_layers`]): one not drawn yet takes no click.
pub fn drawn_popovers(hp: &Headless) -> Vec<Rect> {
    hp.drawn_layers(hocus_link::POPOVER_NAMESPACE)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

/// A screen's refresh, as the tests step the held clock: 60 Hz.
pub const STEP: Duration = Duration::from_millis(16);

/// What `look` reads at each frame for `time`, the motions' clock held
/// and stepped a refresh at a time ([`Headless::step`]), with the time of
/// each since the first step's start. The frames come at the same moments
/// of the motions however slowly a loaded machine draws them. Once nothing
/// moves as of a frame, the frames after would show the same: it stops
/// there. The clock stays held: the caller lets it go.
pub fn sample<T>(
    hp: &mut Headless,
    time: Duration,
    mut look: impl FnMut(&Headless) -> T,
) -> anyhow::Result<Vec<(Duration, T)>> {
    hp.hold_clock();
    let start = hp.now();
    let mut out = Vec::new();
    while hp.now() - start < time {
        hp.step(STEP)?;
        out.push((hp.now() - start, look(hp)));
        if hp.still() {
            break;
        }
    }
    Ok(out)
}

/// Runs the compositor until the last frame drawn shows everything at rest
/// ([`Headless::at_rest`]): what the screen shows is then where the
/// motions end, however late a loaded machine draws it.
pub fn at_rest(hp: &mut Headless) -> anyhow::Result<()> {
    let still = hp.dispatch_until(WAIT, Headless::at_rest)?;
    anyhow::ensure!(still, "the compositor never came to rest");
    Ok(())
}

/// What a program logged to the file at `path`, its whole lines alone
/// and without its colours' escape sequences. A line is written while the
/// test reads: its start, read alone, would be a line that never was (a
/// JSON value cut short), so it counts once it ends.
pub fn logged(path: &Path) -> String {
    let mut bytes = std::fs::read(path).unwrap_or_default();
    let whole = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |at| at + 1);
    bytes.truncate(whole);
    let text = String::from_utf8_lossy(&bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The JSON value of the last line [`logged`] to `path` that holds
/// `marker` (` shows {`), the value from the marker's last character.
pub fn last_json(path: &Path, marker: &str) -> Option<serde_json::Value> {
    let text = logged(path);
    let line = text.lines().rev().find(|l| l.contains(marker))?;
    let at = line.find(marker)? + marker.len() - 1;
    serde_json::from_str(&line[at..]).ok()
}

impl Dispatch<ZwpTextInputManagerV3, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// A writing client's text input: enabled with its caret as its window
/// takes the keyboard, disabled as it loses it, as toolkits do.
impl Dispatch<ZwpTextInputV3, ()> for Client {
    fn event(
        client: &mut Self,
        input: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_text_input_v3::Event::Enter { .. } => {
                input.enable();
                if let Some((x, y, w, h)) = client.caret {
                    input.set_cursor_rectangle(x, y, w, h);
                }
                input.commit();
            }
            zwp_text_input_v3::Event::Leave { .. } => {
                input.disable();
                input.commit();
            }
            zwp_text_input_v3::Event::CommitString { text } => {
                client.committing = text;
            }
            zwp_text_input_v3::Event::Done { .. } => {
                if let Some(text) = client.committing.take() {
                    let _ = client.events.send(Event::Committed(text));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpIdleInhibitManagerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpIdleInhibitManagerV1,
        _: <ZwpIdleInhibitManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpIdleInhibitorV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpIdleInhibitorV1,
        _: <ZwpIdleInhibitorV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinServerDecorationManager, ()> for Client {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinServerDecorationManager,
        _: <OrgKdeKwinServerDecorationManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinServerDecoration, ()> for Client {
    fn event(
        client: &mut Self,
        _: &OrgKdeKwinServerDecoration,
        event: org_kde_kwin_server_decoration::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let org_kde_kwin_server_decoration::Event::Mode { mode } = event {
            let server =
                mode == wayland_client::WEnum::Value(org_kde_kwin_server_decoration::Mode::Server);
            let _ = client.events.send(Event::KdeMode { server });
        }
    }
}

impl Dispatch<ZxdgExporterV2, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZxdgExporterV2,
        _: <ZxdgExporterV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgExportedV2, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ZxdgExportedV2,
        event: zxdg_exported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_exported_v2::Event::Handle { handle } = event {
            let _ = client.events.send(Event::Exported(handle));
        }
    }
}

impl Dispatch<ZxdgImporterV2, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZxdgImporterV2,
        _: <ZxdgImporterV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgImportedV2, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ZxdgImportedV2,
        event: zxdg_imported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_imported_v2::Event::Destroyed = event {
            let _ = client.events.send(Event::ImportGone);
        }
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, bool> for Client {
    fn event(
        client: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        input: &bool,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let input = *input;
        let said = match event {
            ext_idle_notification_v1::Event::Idled => Event::Idled { input },
            ext_idle_notification_v1::Event::Resumed => Event::Resumed { input },
            _ => return,
        };
        let _ = client.events.send(said);
    }
}

// --- Another program's windows (#595) -------------------------------------------------

/// What [`as_other_program`] reads: the compositor's socket, its window's
/// app id, whether it exports it, the handle it opens it as the child of.
const OTHER_SOCKET: &str = "HOCUS_TEST_OTHER_SOCKET";
const OTHER_APP: &str = "HOCUS_TEST_OTHER_APP";
const OTHER_EXPORT: &str = "HOCUS_TEST_OTHER_EXPORT";
const OTHER_PARENT: &str = "HOCUS_TEST_OTHER_PARENT";

/// The body of a test binary's `another_program` test: run by
/// [`other_program`] as a process of its own, a window of another program
/// until the compositor goes (the desktop tells programs apart by their
/// process: a window of the test's own would take the focus as the
/// user's). With [`OTHER_PARENT`], its window is the child of that
/// xdg-foreign handle and asks for the focus with a token of its own, as
/// the desktop portal's backend shows its dialogs; with [`OTHER_EXPORT`],
/// it prints its window's handle (`exported HANDLE`). Run alone, it does
/// nothing.
pub fn as_other_program() -> anyhow::Result<()> {
    let Some(socket) = std::env::var_os(OTHER_SOCKET) else {
        return Ok(());
    };
    let app_id: &'static str = std::env::var(OTHER_APP)
        .unwrap_or_else(|_| "test.other".to_owned())
        .leak();
    let size = (500, 400);
    let client = match std::env::var(OTHER_PARENT) {
        Ok(handle) => {
            let client = client(Path::new(&socket), None)?;
            client.commands.send(Command::OpenDialog {
                app_id,
                size,
                handle,
            })?;
            client.commands.send(Command::Activate(None))?;
            client
        }
        Err(_) => {
            let shows = Shows::Window {
                app_id,
                size,
                popup: false,
            };
            client(Path::new(&socket), Some(shows))?
        }
    };
    if std::env::var_os(OTHER_EXPORT).is_some() {
        client.commands.send(Command::Export)?;
    }
    // Its events end with its connection, as the compositor goes.
    while let Ok(event) = client.events.recv() {
        if let Event::Exported(handle) = event {
            println!("exported {handle}");
        }
    }
    Ok(())
}

/// Another program's window ([`as_other_program`]), stopped by its own PID
/// when dropped; what it prints, line by line.
pub struct OtherProgram {
    child: std::process::Child,
    pub lines: mpsc::Receiver<String>,
}

impl Drop for OtherProgram {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl OtherProgram {
    /// Its window's xdg-foreign handle, once it has printed it.
    pub fn handle(&self, hp: &mut Headless) -> anyhow::Result<String> {
        let mut handle = None;
        let got = hp.dispatch_until(WAIT, |_| {
            if let Ok(line) = self.lines.try_recv()
                && let Some(h) = line.strip_prefix("exported ")
            {
                handle = Some(h.to_owned());
            }
            handle.is_some()
        })?;
        anyhow::ensure!(got, "no handle exported");
        handle.context("no handle")
    }
}

/// Starts another program's window, `app_id`, through the test binary's
/// `another_program` test, and waits until it is mapped: exported or not,
/// the child of `parent` (a handle) or not.
pub fn other_program(
    hp: &mut Headless,
    app_id: &str,
    export: bool,
    parent: Option<&str>,
) -> anyhow::Result<OtherProgram> {
    use std::io::BufRead;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .args(["--exact", "another_program", "--nocapture"])
        .env(OTHER_SOCKET, hp.socket())
        .env(OTHER_APP, app_id)
        .stdout(std::process::Stdio::piped());
    if export {
        command.env(OTHER_EXPORT, "1");
    }
    if let Some(parent) = parent {
        command.env(OTHER_PARENT, parent);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().context("no output")?;
    let (sender, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let other = OtherProgram { child, lines };
    let app = app_id.to_owned();
    let mapped = hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(app.as_str()))
    })?;
    anyhow::ensure!(mapped, "{app_id} never mapped: {:?}", hp.windows());
    Ok(other)
}
