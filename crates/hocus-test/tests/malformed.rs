//! What a client sends never ends the session, in any build (#260): a
//! debug build used to panic on a region of negative size.

use std::{sync::mpsc, time::Duration};

use hocus_compositor::{FRAME, Headless, HeadlessConfig, RendererChoice};
use hocus_test::client::SolidWindow;
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_compositor, wl_region, wl_registry, wl_surface},
};

/// Long enough for a client thread on a loaded CI machine.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

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

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Client {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_surface::WlSurface,
    wl_region::WlRegion
);

/// A surface whose opaque and input regions add a rectangle of height -1
/// and take away one of negative width, then a roundtrip: the compositor
/// read it all, and is still there to answer.
fn send_negative_regions(socket: &std::path::Path) -> anyhow::Result<()> {
    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Client>(&conn)?;
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ())?;
    let surface = compositor.create_surface(&qh, ());
    let region = compositor.create_region(&qh, ());
    region.add(0, 0, 1263, -1);
    region.subtract(10, 10, -20, 5);
    surface.set_opaque_region(Some(&region));
    surface.set_input_region(Some(&region));
    region.destroy();
    surface.commit();
    queue.roundtrip(&mut Client)?;
    Ok(())
}

#[test]
fn a_region_of_negative_size_does_not_end_the_session() -> anyhow::Result<()> {
    let mut compositor = Headless::start(HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..Default::default()
    })?;
    let socket = compositor.socket_path().to_path_buf();
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(send_negative_regions(&socket));
    });
    // The compositor answers while the client waits.
    let mut got = None;
    compositor.dispatch_until(CLIENT_TIMEOUT, |_| {
        got = got.take().or_else(|| result.try_recv().ok());
        got.is_some()
    })?;
    got.expect("the client finished")?;

    // The session goes on: a window still opens and is drawn.
    let window = SolidWindow {
        app_id: "after".into(),
        width: 400,
        height: 300,
        argb: 0xff_33_99_ff,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;
    let before: u64 = compositor.outputs().iter().map(|o| o.frames).sum();
    compositor.advance(FRAME)?;
    let after: u64 = compositor.outputs().iter().map(|o| o.frames).sum();
    assert!(after > before, "no frame drawn after the region");
    Ok(())
}
