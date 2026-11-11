//! A screen unplugged with clients on it (#536, docs/design/windows.md
//! §12, "What clients are told"): every surface on it leaves its
//! `wl_output` before the global goes, its layer surfaces are closed,
//! nothing names that output afterwards, and the windows are configured
//! for the screen they move to. kitty, which drops its `wl_output` on
//! `global_remove`, died of a `wl_surface.leave` naming it after.

use crate::common;

use std::{
    os::{fd::AsFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use common::{WAIT, wait};
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, Placed, RendererChoice, model::Rect};
use smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    protocol::{
        wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_output, wl_output::WlOutput,
        wl_registry, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool, wl_surface,
        wl_surface::WlSurface,
    },
};
use wayland_protocols::{
    wp::fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    xdg::shell::client::{
        xdg_surface::{self, XdgSurface},
        xdg_toplevel::{self, XdgToplevel},
        xdg_wm_base::{self, XdgWmBase},
    },
};

/// The shell's bar's own surfaces, one of each per bar.
const BAR_PARTS: [&str; 8] = [
    hocus_link::NAMESPACE,
    hocus_link::CLOCK_NAMESPACE,
    hocus_link::LEVELS_NAMESPACE,
    hocus_link::CONNECTIONS_NAMESPACE,
    hocus_link::DESKTOPS_NAMESPACE,
    hocus_link::AGENTS_NAMESPACE,
    hocus_link::TASKS_NAMESPACE,
    hocus_link::FOOTER_NAMESPACE,
];
const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";
const APP_ID: &str = "test.watcher";
/// How long a real program (foot, kitty) may take to show its window.
const PROGRAM_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
const LAYER: &str = "test.watcher-layer";

/// 1920×1080 at scale 1, then 2560×1440 at scale 2 on its right: the
/// scales tell which screen a client is told about.
fn screens() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        outputs: vec![
            OutputSpec::new(1920, 1080),
            OutputSpec::new(2560, 1440).scale(2.0),
        ],
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

fn placed(hp: &Headless, app_id: &str) -> Option<Placed> {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
}

fn on(hp: &Headless, app_id: &str) -> Option<String> {
    placed(hp, app_id).and_then(|p| p.output)
}

/// What the watching client was told, in order, and what it should not
/// have been.
#[derive(Debug, Default, Clone)]
struct Report {
    seen: Vec<String>,
    wrong: Vec<String>,
}

impl Report {
    /// Where `line` first is in what was seen.
    fn at(&self, line: &str) -> Option<usize> {
        self.seen.iter().position(|s| s == line)
    }

    fn after(&self, at: usize) -> &[String] {
        &self.seen[at + 1..]
    }
}

/// A client that binds every `wl_output` and keeps them (as a client that
/// had not read `global_remove` yet would), with a window and a layer
/// surface on the second screen, and checks everything it is told about
/// the outputs.
struct Watching {
    report: Arc<Mutex<Report>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watching {
    fn start(socket: &Path) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        stream.set_nonblocking(true)?;
        let conn = Connection::from_socket(stream)?;
        let report = Arc::new(Mutex::new(Report::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new().name("watcher".into()).spawn({
            let report = report.clone();
            let stop = stop.clone();
            move || {
                if let Err(err) = watch(&conn, &report, &stop) {
                    eprintln!("watcher: {err:#}");
                    if let Ok(mut r) = report.lock() {
                        r.wrong.push(format!("the client failed: {err:#}"));
                    }
                }
            }
        })?;
        Ok(Self {
            report,
            stop,
            thread: Some(thread),
        })
    }

    fn report(&self) -> Report {
        self.report.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn has_seen(&self, line: &str) -> bool {
        self.report().at(line).is_some()
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Output {
    proxy: WlOutput,
    global: u32,
    name: Option<String>,
    removed: bool,
}

struct Shown {
    surface: WlSurface,
    role: &'static str,
    entered: Vec<WlOutput>,
}

struct Watcher {
    report: Arc<Mutex<Report>>,
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    layer_shell: Option<ZwlrLayerShellV1>,
    fractional: Option<WpFractionalScaleManagerV1>,
    outputs: Vec<Output>,
    shown: Vec<Shown>,
    toplevel: Option<(WlSurface, XdgSurface, XdgToplevel)>,
    layer: Option<(WlSurface, ZwlrLayerSurfaceV1)>,
    /// The size the toplevel was last configured to, 0 for its own.
    size: (i32, i32),
}

impl Watcher {
    fn note(&self, line: String) {
        if let Ok(mut r) = self.report.lock() {
            r.seen.push(line);
        }
    }

    fn wrong(&self, line: String) {
        eprintln!("watcher: {line}");
        if let Ok(mut r) = self.report.lock() {
            r.wrong.push(line);
        }
    }

    fn output(&self, proxy: &WlOutput) -> Option<&Output> {
        self.outputs.iter().find(|o| o.proxy == *proxy)
    }

    fn output_name(&self, proxy: &WlOutput) -> String {
        self.output(proxy)
            .and_then(|o| o.name.clone())
            .unwrap_or_else(|| format!("{}", proxy.id()))
    }

    fn role(&self, surface: &WlSurface) -> &'static str {
        self.shown
            .iter()
            .find(|s| s.surface == *surface)
            .map_or("unknown", |s| s.role)
    }

    /// A `w`×`h` buffer, of no colour in particular.
    fn buffer(&self, qh: &QueueHandle<Self>, w: i32, h: i32) -> anyhow::Result<WlBuffer> {
        let shm = self.shm.as_ref().context("no wl_shm")?;
        let file = tempfile::tempfile()?;
        file.set_len(u64::try_from(w * h * 4)?)?;
        let pool = shm.create_pool(file.as_fd(), w * h * 4, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Xrgb8888, qh, ());
        pool.destroy();
        Ok(buffer)
    }

    fn draw(&self, qh: &QueueHandle<Self>, surface: &WlSurface, (w, h): (i32, i32)) {
        match self.buffer(qh, w, h) {
            Ok(buffer) => {
                surface.attach(Some(&buffer), 0, 0);
                surface.damage_buffer(0, 0, w, h);
                surface.commit();
            }
            Err(err) => self.wrong(format!("could not draw: {err:#}")),
        }
    }

    fn show(&mut self, qh: &QueueHandle<Self>) -> anyhow::Result<()> {
        let compositor = self.compositor.clone().context("no wl_compositor")?;
        let wm_base = self.wm_base.clone().context("no xdg_wm_base")?;
        let layer_shell = self.layer_shell.clone().context("no layer shell")?;
        let surface = compositor.create_surface(qh, ());
        if let Some(fractional) = &self.fractional {
            fractional.get_fractional_scale(&surface, qh, ());
        }
        let xdg = wm_base.get_xdg_surface(&surface, qh, ());
        let toplevel = xdg.get_toplevel(qh, ());
        toplevel.set_app_id(APP_ID.into());
        surface.commit();
        self.shown.push(Shown {
            surface: surface.clone(),
            role: "toplevel",
            entered: Vec::new(),
        });
        self.toplevel = Some((surface, xdg, toplevel));
        let second = self
            .outputs
            .iter()
            .find(|o| o.name.as_deref() == Some(SECOND))
            .map(|o| o.proxy.clone())
            .context("no second output")?;
        let surface = compositor.create_surface(qh, ());
        let layer = layer_shell.get_layer_surface(
            &surface,
            Some(&second),
            zwlr_layer_shell_v1::Layer::Top,
            LAYER.into(),
            qh,
            (),
        );
        layer.set_size(200, 30);
        layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top);
        surface.commit();
        self.shown.push(Shown {
            surface: surface.clone(),
            role: "layer",
            entered: Vec::new(),
        });
        self.layer = Some((surface, layer));
        Ok(())
    }
}

fn watch(conn: &Connection, report: &Arc<Mutex<Report>>, stop: &AtomicBool) -> anyhow::Result<()> {
    let mut queue: EventQueue<Watcher> = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut watcher = Watcher {
        report: report.clone(),
        compositor: None,
        shm: None,
        wm_base: None,
        layer_shell: None,
        fractional: None,
        outputs: Vec::new(),
        shown: Vec::new(),
        toplevel: None,
        layer: None,
        size: (0, 0),
    };
    // The globals, then what the outputs say of themselves.
    queue.roundtrip(&mut watcher)?;
    queue.roundtrip(&mut watcher)?;
    watcher.show(&qh)?;
    while !stop.load(Ordering::Relaxed) {
        queue.dispatch_pending(&mut watcher)?;
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
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for Watcher {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "xdg_wm_base" => state.wm_base = Some(registry.bind(name, version.min(5), qh, ())),
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wp_fractional_scale_manager_v1" => {
                    state.fractional = Some(registry.bind(name, 1, qh, ()));
                }
                "wl_output" => {
                    let proxy = registry.bind(name, version.min(4), qh, ());
                    state.outputs.push(Output {
                        proxy,
                        global: name,
                        name: None,
                        removed: false,
                    });
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                let Some(at) = state.outputs.iter().position(|o| o.global == name) else {
                    return;
                };
                let proxy = state.outputs[at].proxy.clone();
                let output = state.output_name(&proxy);
                for shown in &state.shown {
                    if shown.entered.contains(&proxy) {
                        state.wrong(format!(
                            "global_remove of {output} while the {} is still on it",
                            shown.role
                        ));
                    }
                }
                state.outputs[at].removed = true;
                state.note(format!("global_remove {output}"));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for Watcher {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.output(proxy).is_some_and(|o| o.removed) {
            let output = state.output_name(proxy);
            state.wrong(format!("{output}, removed, sends {event:?}"));
        }
        if let wl_output::Event::Name { name } = event {
            if let Some(o) = state.outputs.iter_mut().find(|o| o.proxy == *proxy) {
                o.name = Some(name.clone());
            }
            state.note(format!("global {name}"));
        }
    }
}

impl Dispatch<WlSurface, ()> for Watcher {
    fn event(
        state: &mut Self,
        surface: &WlSurface,
        event: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let role = state.role(surface);
        match event {
            wl_surface::Event::Enter { output } | wl_surface::Event::Leave { output }
                if state.output(&output).is_some_and(|o| o.removed) =>
            {
                let name = state.output_name(&output);
                state.wrong(format!("the {role} is told of {name}, removed"));
            }
            wl_surface::Event::Enter { output } => {
                let name = state.output_name(&output);
                if let Some(s) = state.shown.iter_mut().find(|s| s.surface == *surface) {
                    s.entered.push(output);
                }
                state.note(format!("{role} enter {name}"));
            }
            wl_surface::Event::Leave { output } => {
                let name = state.output_name(&output);
                if let Some(s) = state.shown.iter_mut().find(|s| s.surface == *surface) {
                    s.entered.retain(|o| *o != output);
                }
                state.note(format!("{role} leave {name}"));
            }
            wl_surface::Event::PreferredBufferScale { factor } => {
                state.note(format!("{role} buffer-scale {factor}"));
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Watcher {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for Watcher {
    fn event(
        state: &mut Self,
        xdg: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            let Some((surface, ..)) = state.toplevel.clone() else {
                return;
            };
            let size = match state.size {
                (0, _) | (_, 0) => (400, 300),
                size => size,
            };
            state.draw(qh, &surface, size);
        }
    }
}

impl Dispatch<XdgToplevel, ()> for Watcher {
    fn event(
        state: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                state.size = (width, height);
                state.note(format!("toplevel configure {width}x{height}"));
            }
            xdg_toplevel::Event::ConfigureBounds { width, height } => {
                state.note(format!("toplevel bounds {width}x{height}"));
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for Watcher {
    fn event(
        state: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                let Some((surface, _)) = state.layer.clone() else {
                    return;
                };
                let w = i32::try_from(width).unwrap_or(0).max(1);
                let h = i32::try_from(height).unwrap_or(0).max(1);
                state.draw(qh, &surface, (w, h));
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.note("layer closed".into());
                // As the protocol asks: it is destroyed.
                if let Some((surface, layer)) = state.layer.take() {
                    layer.destroy();
                    state.shown.retain(|s| s.surface != surface);
                    surface.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for Watcher {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.note(format!("toplevel fractional {scale}"));
        }
    }
}

delegate_noop!(Watcher: WlCompositor);
delegate_noop!(Watcher: ignore WlShm);
delegate_noop!(Watcher: WlShmPool);
delegate_noop!(Watcher: ignore WlBuffer);
delegate_noop!(Watcher: ZwlrLayerShellV1);
delegate_noop!(Watcher: WpFractionalScaleManagerV1);

/// The compositor with the watcher's window and layer surface on the
/// second screen.
fn watched() -> anyhow::Result<(Headless, Watching)> {
    let mut hp = screens()?;
    // The second screen is 1280×720 logical, from x 1920.
    hp.pointer_move(1920.0 + 640.0, 360.0);
    let watching = Watching::start(hp.socket())?;
    let shown = hp.dispatch_until(WAIT, |hp| {
        on(hp, APP_ID).as_deref() == Some(SECOND)
            && hp.layer_rects(LAYER).iter().any(|(o, _)| o == SECOND)
            && watching.has_seen("toplevel enter HEADLESS-2")
            && watching.has_seen("layer enter HEADLESS-2")
    })?;
    assert!(
        shown,
        "the watcher on the second screen: {:#?}",
        watching.report()
    );
    Ok((hp, watching))
}

#[test]
fn a_screen_unplugged_its_surfaces_leave_it_before_its_global_goes() -> anyhow::Result<()> {
    let (mut hp, watching) = watched()?;
    let area = hp.work_area_of(FIRST).context("the first's work area")?;

    hp.unplug(SECOND)?;
    let moved = hp.dispatch_until(WAIT, |hp| {
        on(hp, APP_ID).as_deref() == Some(FIRST) && watching.has_seen("toplevel enter HEADLESS-1")
    })?;
    // Until the client has been told all of the move: the second's global
    // gone, the first's bounds and scale. On a loaded machine they come
    // over several turns of its loop.
    let first_bounds = format!("toplevel bounds {}x{}", area.width, area.height);
    let last = |seen: &[String], what: &str| {
        seen.iter()
            .rfind(|s| s.starts_with(what))
            .cloned()
            .unwrap_or_default()
    };
    hp.dispatch_until(WAIT, |_| {
        let report = watching.report();
        report.at("global_remove HEADLESS-2").is_some()
            && last(&report.seen, "toplevel bounds ") == first_bounds
            && last(&report.seen, "toplevel fractional ") == "toplevel fractional 120"
    })?;
    let report = watching.report();
    assert!(moved, "the window moved to the first screen: {report:#?}");
    assert!(report.wrong.is_empty(), "{report:#?}");
    let removed = report
        .at("global_remove HEADLESS-2")
        .with_context(|| format!("the second's global went: {report:#?}"))?;
    // Its surfaces left it, the layer surface was closed, before.
    let left = report.at("toplevel leave HEADLESS-2");
    assert!(left.is_some_and(|l| l < removed), "{report:#?}");
    let layer_left = report.at("layer leave HEADLESS-2");
    assert!(layer_left.is_some_and(|l| l < removed), "{report:#?}");
    let closed = report.at("layer closed");
    assert!(closed.is_some_and(|c| c < removed), "{report:#?}");
    // The window was configured for the first screen as it moved.
    let left = left.unwrap_or_default();
    let configured = report
        .after(left)
        .iter()
        .any(|s| s.starts_with("toplevel configure "));
    assert!(configured, "a configure after leaving: {report:#?}");
    let bounds = report
        .seen
        .iter()
        .rfind(|s| s.starts_with("toplevel bounds "));
    assert_eq!(bounds, Some(&first_bounds), "{report:#?}");
    // It is told the first screen's scale (1), never the second's (2)
    // again (wl_surface's preferred scale too, should the compositor
    // speak wl_compositor 6).
    for line in report.after(removed) {
        assert_ne!(line, "toplevel buffer-scale 2", "{report:#?}");
        assert_ne!(line, "toplevel fractional 240", "{report:#?}");
    }
    let fractional = report
        .seen
        .iter()
        .rfind(|s| s.starts_with("toplevel fractional "));
    assert_eq!(
        fractional.map(String::as_str),
        Some("toplevel fractional 120")
    );
    assert_eq!(hp.layer_rects(LAYER), Vec::new(), "the layer surface went");
    Ok(())
}

#[test]
fn a_screen_plugged_back_takes_its_window_and_the_old_output_stays_quiet() -> anyhow::Result<()> {
    let (mut hp, watching) = watched()?;
    hp.unplug(SECOND)?;
    let moved = hp.dispatch_until(WAIT, |hp| {
        on(hp, APP_ID).as_deref() == Some(FIRST) && watching.has_seen("toplevel enter HEADLESS-1")
    })?;
    assert!(moved, "{:#?}", watching.report());
    wait(&mut hp, Duration::from_millis(100))?;

    hp.plug(SECOND)?;
    // Back where it was (§12): it was not placed elsewhere meanwhile.
    let back = hp.dispatch_until(WAIT, |hp| {
        let report = watching.report();
        on(hp, APP_ID).as_deref() == Some(SECOND)
            && report.at("global_remove HEADLESS-2").is_some_and(|r| {
                report
                    .after(r)
                    .iter()
                    .any(|s| s == "toplevel enter HEADLESS-2")
            })
    })?;
    wait(&mut hp, Duration::from_millis(200))?;
    let report = watching.report();
    assert!(back, "the window back on the second screen: {report:#?}");
    // Only the new output was named: the old one, kept by the client,
    // was told nothing more.
    assert!(report.wrong.is_empty(), "{report:#?}");
    let removed = report.at("global_remove HEADLESS-2").unwrap_or_default();
    let again = report
        .after(removed)
        .iter()
        .any(|s| s == "global HEADLESS-2");
    assert!(again, "the second's output again: {report:#?}");
    Ok(())
}

/// `program`'s path on `PATH`, if it is installed.
fn installed(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

/// A program started for a test, stopped (by its own PID) when it ends.
struct Started(Child);

impl Drop for Started {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// `program` (a terminal) on the second screen keeps running as it is
/// unplugged, its window going to the first, and when it is plugged
/// back, its window going back. Fails when it is not installed: CI's
/// machines have neither, and leave these tests out by name
/// (`.config/nextest.toml`).
fn a_terminal_survives(program: &str, args: &[&str], app_id: &str) -> anyhow::Result<()> {
    let Some(path) = installed(program) else {
        common::missing(program);
    };
    let mut hp = screens()?;
    hp.pointer_move(1920.0 + 640.0, 360.0);
    // Away from the session it runs in: homes of its own, no bus, its
    // drawing on the CPU (kitty's GL), its protocol written down.
    let dir = tempfile::Builder::new().prefix("hp").tempdir_in("/tmp")?;
    let mut env = Vec::new();
    for (var, sub) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_RUNTIME_DIR", "run"),
    ] {
        let place = dir.path().join(sub);
        std::fs::create_dir(&place)?;
        env.push((var, place));
    }
    let log = dir.path().join("protocol.log");
    let out = std::fs::File::create(&log)?;
    let child = Command::new(&path)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(env)
        .env("WAYLAND_DISPLAY", hp.socket())
        .env("WAYLAND_DEBUG", "1")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("GALLIUM_DRIVER", "llvmpipe")
        .stdout(out.try_clone()?)
        .stderr(out)
        .spawn()
        .with_context(|| format!("could not start {program}"))?;
    let mut child = Started(child);
    let tail = |log: &Path| {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(40)..].join("\n")
    };
    // A real program starting: on a loaded machine, with its GL on the
    // CPU, well over the tests' usual wait.
    let shown = hp.dispatch_until(PROGRAM_WAIT, |hp| on(hp, app_id).as_deref() == Some(SECOND))?;
    assert!(shown, "{program} on the second screen: {}", tail(&log));
    wait(&mut hp, Duration::from_millis(300))?;

    hp.unplug(SECOND)?;
    let moved = hp.dispatch_until(WAIT, |hp| on(hp, app_id).as_deref() == Some(FIRST))?;
    wait(&mut hp, Duration::from_millis(500))?;
    let status = child.0.try_wait()?;
    assert_eq!(status, None, "{program} ended: {}", tail(&log));
    assert!(moved, "{program}'s window on the first screen");

    hp.plug(SECOND)?;
    let back = hp.dispatch_until(WAIT, |hp| on(hp, app_id).as_deref() == Some(SECOND))?;
    wait(&mut hp, Duration::from_millis(500))?;
    let status = child.0.try_wait()?;
    assert_eq!(status, None, "{program} ended: {}", tail(&log));
    assert!(back, "{program}'s window back on the second screen");
    Ok(())
}

#[test]
fn kitty_keeps_running_on_an_unplugged_screen() -> anyhow::Result<()> {
    a_terminal_survives(
        "kitty",
        &[
            "--config",
            "NONE",
            "-o",
            "linux_display_server=wayland",
            "sleep",
            "600",
        ],
        "kitty",
    )
}

#[test]
fn foot_keeps_running_on_an_unplugged_screen() -> anyhow::Result<()> {
    a_terminal_survives("foot", &["sleep", "600"], "foot")
}

/// A client that asks for layer surfaces of `namespaces` on no screen,
/// in that order, and notes which are closed.
struct Asking {
    closed: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Asking {
    fn start(socket: &Path, namespaces: &[&str]) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        stream.set_nonblocking(true)?;
        let conn = Connection::from_socket(stream)?;
        let closed = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let namespaces: Vec<String> = namespaces.iter().map(|n| (*n).to_owned()).collect();
        let thread = std::thread::Builder::new().name("asker".into()).spawn({
            let closed = closed.clone();
            let stop = stop.clone();
            move || {
                if let Err(err) = ask(&conn, &namespaces, &closed, &stop) {
                    eprintln!("asker: {err:#}");
                }
            }
        })?;
        Ok(Self {
            closed,
            stop,
            thread: Some(thread),
        })
    }

    fn closed(&self) -> Vec<String> {
        self.closed.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl Drop for Asking {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
struct Asker {
    compositor: Option<WlCompositor>,
    layer_shell: Option<ZwlrLayerShellV1>,
    closed: Arc<Mutex<Vec<String>>>,
}

fn ask(
    conn: &Connection,
    namespaces: &[String],
    closed: &Arc<Mutex<Vec<String>>>,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    let mut queue: EventQueue<Asker> = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut asker = Asker {
        closed: closed.clone(),
        ..Asker::default()
    };
    queue.roundtrip(&mut asker)?;
    let compositor = asker.compositor.clone().context("no wl_compositor")?;
    let layer_shell = asker.layer_shell.clone().context("no layer shell")?;
    let mut kept = Vec::new();
    for namespace in namespaces {
        let surface = compositor.create_surface(&qh, ());
        let layer = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            namespace.clone(),
            &qh,
            namespace.clone(),
        );
        layer.set_size(200, 30);
        layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top);
        surface.commit();
        // One at a time: each sees where the ones before it went.
        queue.roundtrip(&mut asker)?;
        kept.push((surface, layer));
    }
    while !stop.load(Ordering::Relaxed) {
        queue.dispatch_pending(&mut asker)?;
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
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for Asker {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, String> for Asker {
    fn event(
        state: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        namespace: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => layer.ack_configure(serial),
            zwlr_layer_surface_v1::Event::Closed => {
                if let Ok(mut closed) = state.closed.lock() {
                    closed.push(namespace.clone());
                }
            }
            _ => {}
        }
    }
}

delegate_noop!(Asker: WlCompositor);
delegate_noop!(Asker: ignore WlSurface);
delegate_noop!(Asker: ZwlrLayerShellV1);

/// A bar's surface asked for on no screen goes to the screen given a bar
/// that lacks it, not under the pointer; a second one, with no screen
/// left that lacks it, is closed; any other surface is under the pointer
/// (#540: the shell may ask before it knows a screen's name).
#[test]
fn a_bar_surface_asked_on_no_screen_goes_to_the_screen_lacking_it() -> anyhow::Result<()> {
    let mut hp = screens()?;
    assert_eq!(
        hp.bars().iter().map(|b| b.0.as_str()).collect::<Vec<_>>(),
        [FIRST],
        "one bar, on the first screen"
    );
    let second = hp
        .outputs()
        .into_iter()
        .find(|(n, _)| n == SECOND)
        .map(|(_, r)| r)
        .context("the second screen")?;
    hp.pointer_move(
        f64::from(second.x + second.width / 2),
        f64::from(second.y + second.height / 2),
    );
    let other = "test.other";
    let asking = Asking::start(
        hp.socket(),
        &[
            hocus_link::NAMESPACE,
            hocus_link::CLOCK_NAMESPACE,
            hocus_link::NAMESPACE,
            other,
        ],
    )?;
    let on = |hp: &Headless, namespace: &str| -> Vec<String> {
        hp.layer_rects(namespace)
            .into_iter()
            .map(|(o, _)| o)
            .collect()
    };
    let placed = hp.dispatch_until(WAIT, |hp| {
        !on(hp, other).is_empty() && asking.closed() == [hocus_link::NAMESPACE]
    })?;
    assert!(
        placed,
        "the surfaces placed: {:?} {:?} {:?}, closed {:?}",
        on(&hp, hocus_link::NAMESPACE),
        on(&hp, hocus_link::CLOCK_NAMESPACE),
        on(&hp, other),
        asking.closed()
    );
    assert_eq!(on(&hp, hocus_link::NAMESPACE), [FIRST], "the bar");
    assert_eq!(on(&hp, hocus_link::CLOCK_NAMESPACE), [FIRST], "its clock");
    assert_eq!(on(&hp, other), [SECOND], "the other, under the pointer");
    assert_eq!(hp.bar_parts_asked_nowhere(), 3, "the bar's, counted");
    Ok(())
}

/// The shell's bar on the screen unplugged is closed there and drawn on
/// the screen left; plugged back, the bar is that screen's again. The
/// shell stays linked throughout.
#[test]
fn the_shells_bar_closed_with_its_screen_is_drawn_on_the_other() -> anyhow::Result<()> {
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let mut env = vec![("ICED_BACKEND", "tiny-skia".to_owned())];
    for (var, sub) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
    ] {
        let place = dir.path().join(sub);
        std::fs::create_dir(&place)?;
        env.push((var, place.display().to_string()));
    }
    let shell = common::shell_script(dir.path(), &shell, &env)?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(1920, 1080)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        hocus_compositor::Config::default(),
        Some(shell),
    )?;
    let bar_on =
        |hp: &Headless| -> Vec<String> { hp.bar_surfaces().into_iter().map(|(o, _)| o).collect() };
    // The screens of each of the bar's surfaces. Its first one shows before
    // the shell has asked for the others: its screen unplugged then, the
    // shell's toolkit no longer knows the name they are asked for by, and
    // asks for them on no screen (#751). So the test waits for the whole
    // bar before it unplugs a screen.
    let whole_on = |hp: &Headless| -> Vec<Vec<String>> {
        BAR_PARTS
            .iter()
            .map(|n| hp.layer_rects(n).into_iter().map(|(o, _)| o).collect())
            .collect()
    };
    let only = |screen: &str| vec![vec![screen.to_owned()]; BAR_PARTS.len()];
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.shell_connected() && whole_on(hp) == only(FIRST)
    })?;
    assert!(drawn, "the bar on the first screen: {:?}", whole_on(&hp));

    hp.unplug(FIRST)?;
    let moved = hp.dispatch_until(WAIT, |hp| whole_on(hp) == only(SECOND))?;
    assert!(moved, "the bar on the second screen: {:?}", whole_on(&hp));
    wait(&mut hp, Duration::from_millis(300))?;
    assert!(hp.shell_connected(), "the shell went");

    // Plugged back with the pointer on the other screen: the bar is the
    // screen's that came back (#540), never the one under the pointer,
    // which the compositor would give a bar asked for on no screen.
    let screen = |hp: &Headless, name: &str| {
        hp.outputs()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r)
    };
    let second = screen(&hp, SECOND).context("the second screen")?;
    hp.pointer_move(
        f64::from(second.x + second.width / 2),
        f64::from(second.y + second.height / 2),
    );
    hp.plug(FIRST)?;
    let first = screen(&hp, FIRST).context("the first screen, back")?;
    let (x, y) = hp.pointer();
    let inside = |r: Rect| {
        x >= f64::from(r.x)
            && x < f64::from(r.x + r.width)
            && y >= f64::from(r.y)
            && y < f64::from(r.y + r.height)
    };
    assert!(
        inside(second) && !inside(first),
        "the pointer on the second screen: {:?} {:?}",
        hp.pointer(),
        hp.outputs()
    );
    // The second screen's bar goes as the first one's comes: one bar,
    // on the first screen, and it stays there.
    let back = hp.dispatch_until(WAIT, |hp| {
        hp.bars().iter().map(|b| b.0.as_str()).eq([FIRST]) && bar_on(hp) == [FIRST]
    })?;
    assert!(
        back,
        "one bar, on the screen that came back: {:?} {:?}",
        hp.bars(),
        hp.bar_surfaces()
    );
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(
        bar_on(&hp),
        [FIRST],
        "the bar stays on the screen that came back: {:?}",
        hp.bar_surfaces()
    );
    // The shell named the screen itself: the compositor did not have to
    // find one.
    assert_eq!(
        hp.bar_parts_asked_nowhere(),
        0,
        "the bar's surfaces asked for on no screen"
    );
    assert!(hp.shell_connected(), "the shell went");
    Ok(())
}
