//! The protocols of the common batch (#32) are offered, and a client
//! started through a security context (a sandbox) does not see those that
//! read or drive the rest of the session.

use std::{
    os::{fd::AsFd, unix::net::UnixListener},
    path::Path,
    sync::mpsc,
    time::Duration,
};

use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1::WpSecurityContextManagerV1,
    wp_security_context_v1::WpSecurityContextV1,
};

/// The protocols of the batch.
const BATCH: [&str; 18] = [
    "xdg_activation_v1",
    "zwp_idle_inhibit_manager_v1",
    "wp_cursor_shape_manager_v1",
    "zwp_keyboard_shortcuts_inhibit_manager_v1",
    "zxdg_exporter_v2",
    "zxdg_importer_v2",
    "wp_presentation",
    "wp_fractional_scale_manager_v1",
    "wp_viewporter",
    "wp_single_pixel_buffer_manager_v1",
    "wp_alpha_modifier_v1",
    "wp_content_type_manager_v1",
    "zwp_pointer_constraints_v1",
    "zwp_relative_pointer_manager_v1",
    "zwp_pointer_gestures_v1",
    "zwp_primary_selection_device_manager_v1",
    "ext_data_control_manager_v1",
    "wp_security_context_manager_v1",
];

/// What a sandboxed client must not see.
const SENSITIVE: [&str; 5] = [
    "zwlr_layer_shell_v1",
    "zwp_virtual_keyboard_manager_v1",
    "ext_data_control_manager_v1",
    "ext_foreign_toplevel_list_v1",
    "wp_security_context_manager_v1",
];

struct Client;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Client {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextManagerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextManagerV1,
        _: <WpSecurityContextManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextV1,
        _: <WpSecurityContextV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn interfaces(socket: &Path) -> anyhow::Result<Vec<String>> {
    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, _queue) = registry_queue_init::<Client>(&conn)?;
    Ok(globals
        .contents()
        .clone_list()
        .into_iter()
        .map(|g| g.interface)
        .collect())
}

/// What a client of the compositor at `socket` sees, then what one
/// started through a security context sees.
fn both(socket: &Path, dir: &Path) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let plain = interfaces(socket)?;
    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Client>(&conn)?;
    let qh = queue.handle();
    let manager: WpSecurityContextManagerV1 = globals.bind(&qh, 1..=1, ())?;
    let path = dir.join("sandbox");
    let listener = UnixListener::bind(&path)?;
    // The sandbox lives until this end closes.
    let (close_read, _close_write) = rustix::pipe::pipe()?;
    let context = manager.create_listener(listener.as_fd(), close_read.as_fd(), &qh, ());
    context.set_sandbox_engine("org.hocuspanda.test".into());
    context.set_app_id("sandboxed".into());
    context.commit();
    queue.roundtrip(&mut Client)?;
    let sandboxed = interfaces(&path)?;
    Ok((plain, sandboxed))
}

#[test]
fn a_sandboxed_client_does_not_see_the_sensitive_protocols() -> anyhow::Result<()> {
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: false,
        session_bus: None,
        data_dirs: None,
        child_env: None,
    })?;
    let socket = compositor.socket_path().to_path_buf();
    let dir = tempfile::tempdir()?;
    let dir_path = dir.path().to_path_buf();
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(both(&socket, &dir_path));
    });
    // The compositor answers while the clients wait.
    let mut got = None;
    compositor.dispatch_until(Duration::from_secs(10), |_| {
        got = got.take().or_else(|| result.try_recv().ok());
        got.is_some()
    })?;
    let (plain, sandboxed) = got.expect("the clients finished")?;
    for interface in BATCH.iter().chain(&SENSITIVE) {
        assert!(
            plain.iter().any(|i| i == interface),
            "{interface} missing: {plain:?}"
        );
    }
    for interface in SENSITIVE {
        assert!(
            !sandboxed.iter().any(|i| i == interface),
            "{interface} offered to a sandbox"
        );
    }
    for interface in ["wl_compositor", "xdg_wm_base", "wp_viewporter"] {
        assert!(
            sandboxed.iter().any(|i| i == interface),
            "{interface} missing in a sandbox"
        );
    }
    Ok(())
}
