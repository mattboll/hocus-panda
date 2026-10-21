//! Acceptance scenarios of the control API (#6). Each test talks to its own
//! headless compositor over the control socket, from a client thread, while
//! the test thread runs the compositor's loop.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    thread::JoinHandle,
    time::Duration,
};

use hocus_api::{
    Button, Client, ClientError, ErrorCode, Event, Permission, Rect, Reply, Request, Target,
    WindowInfo,
};
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice};
use hocus_test::client::SolidWindow;

const TIMEOUT: Duration = Duration::from_secs(10);

fn back() -> SolidWindow {
    SolidWindow {
        app_id: "test.back".into(),
        width: 400,
        height: 300,
        argb: 0xff_33_99_ff,
    }
}

fn front() -> SolidWindow {
    SolidWindow {
        app_id: "test.front".into(),
        width: 200,
        height: 100,
        argb: 0xff_ff_66_00,
    }
}

fn compositor(permission: Permission) -> anyhow::Result<Headless> {
    Headless::start_with(
        HeadlessConfig {
            outputs: vec![
                OutputSpec::new(2560, 1600).scale(2.0),
                OutputSpec::new(1920, 1080),
            ],
            renderer: RendererChoice::Pixman,
        },
        permission,
    )
}

/// Opens `windows` one after the other, so that the last one is on top.
fn open(compositor: &mut Headless, windows: &[SolidWindow]) -> anyhow::Result<()> {
    for (n, window) in windows.iter().enumerate() {
        let _client = window.clone().spawn(compositor.socket_path())?;
        compositor.dispatch_until(TIMEOUT, |c| c.window_count() == n + 1)?;
    }
    Ok(())
}

/// Runs `talk` with a control client in its own thread while the compositor
/// serves it.
fn with_client<T: Send + 'static>(
    compositor: &mut Headless,
    talk: impl FnOnce(&mut Client) -> Result<T, ClientError> + Send + 'static,
) -> anyhow::Result<T> {
    let socket: PathBuf = compositor.api_socket().to_path_buf();
    let thread: JoinHandle<Result<T, ClientError>> = std::thread::spawn(move || {
        let mut client = Client::connect(&socket)?;
        client.set_timeout(Some(TIMEOUT))?;
        talk(&mut client)
    });
    compositor.dispatch_until(TIMEOUT, |_| thread.is_finished())?;
    let result = thread
        .join()
        .map_err(|_| anyhow::anyhow!("the client thread panicked"))?;
    Ok(result?)
}

fn windows(client: &mut Client) -> Result<Vec<WindowInfo>, ClientError> {
    match client.request(Request::Windows)? {
        Reply::Windows(windows) => Ok(windows),
        other => Err(ClientError::Protocol(format!("{other:?}"))),
    }
}

#[test]
fn lists_a_window_with_its_app_id_output_and_geometry() -> anyhow::Result<()> {
    let mut compositor = compositor(Permission::Observe)?;
    open(&mut compositor, &[back()])?;

    let windows = with_client(&mut compositor, windows)?;

    let [window] = windows.as_slice() else {
        anyhow::bail!("expected one window, got {windows:?}");
    };
    assert_eq!(window.app_id.as_deref(), Some("test.back"));
    assert_eq!(window.output.as_deref(), Some("HEADLESS-1"));
    // Centred on the first output, 1280×800 logical.
    assert_eq!(
        window.geometry,
        Rect {
            x: 440,
            y: 250,
            width: 400,
            height: 300
        }
    );
    assert!(window.focused);
    Ok(())
}

#[test]
fn a_click_on_the_back_window_focuses_it() -> anyhow::Result<()> {
    let mut compositor = compositor(Permission::Test)?;
    open(&mut compositor, &[back(), front()])?;

    let (back, events, scene) = with_client(&mut compositor, |client| {
        let windows = windows(client)?;
        let back = windows
            .iter()
            .find(|w| w.app_id.as_deref() == Some("test.back"))
            .map(|w| w.id)
            .ok_or_else(|| ClientError::Protocol("no back window".into()))?;
        client.request(Request::Subscribe)?;
        // Inside the back window, outside the front one.
        client.request(Request::PointerMove { x: 450.0, y: 260.0 })?;
        for pressed in [true, false] {
            client.request(Request::PointerButton {
                button: Button::Left,
                pressed,
            })?;
        }
        let focus = client.next_event()?;
        let scene = client.request(Request::Scene)?;
        Ok((back, focus, scene))
    })?;

    assert_eq!(events, Event::WindowFocused { id: Some(back) });
    let Reply::Scene(scene) = scene else {
        anyhow::bail!("expected a scene, got {scene:?}");
    };
    assert_eq!(scene.focused, Some(back));
    let stack: Vec<_> = scene.outputs[0].layers[0]
        .windows
        .iter()
        .map(|w| (w.app_id.clone().unwrap_or_default(), w.focused))
        .collect();
    assert_eq!(
        stack,
        [("test.front".into(), false), ("test.back".into(), true)],
        "the clicked window is raised and focused"
    );
    Ok(())
}

#[test]
fn input_injection_is_refused_without_the_test_permission() -> anyhow::Result<()> {
    let mut compositor = compositor(Permission::Observe)?;

    let refused = with_client(&mut compositor, |client| {
        Ok(client.request(Request::PointerMove { x: 1.0, y: 1.0 }))
    })?;

    let Err(ClientError::Api(error)) = refused else {
        anyhow::bail!("expected a refusal, got {refused:?}");
    };
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(
        error.message.contains("--api-permission test"),
        "the message says how to get the permission: {}",
        error.message
    );
    Ok(())
}

#[test]
fn a_screenshot_of_an_output_is_a_png_of_its_pixel_size() -> anyhow::Result<()> {
    let mut compositor = compositor(Permission::Observe)?;
    open(&mut compositor, &[back()])?;

    let reply = with_client(&mut compositor, |client| {
        client.request(Request::Screenshot {
            target: Target::Output("HEADLESS-1".into()),
            cursor: false,
        })
    })?;

    let Reply::Screenshot(shot) = reply else {
        anyhow::bail!("expected a screenshot, got {reply:?}");
    };
    let image = decode(&shot.png()?)?;
    assert_eq!((image.width, image.height), (2560, 1600));
    // The window's colour at the centre, the background in the corner.
    assert_eq!(image.pixel(1280, 800), [0x33, 0x99, 0xff, 0xff]);
    assert_eq!(image.pixel(10, 10), [14, 15, 18, 0xff]);
    Ok(())
}

#[test]
fn a_screenshot_of_a_window_has_its_size_on_its_output() -> anyhow::Result<()> {
    let mut compositor = compositor(Permission::Observe)?;
    open(&mut compositor, &[back()])?;

    let reply = with_client(&mut compositor, |client| {
        let id = windows(client)?.first().map(|w| w.id).unwrap_or_default();
        client.request(Request::Screenshot {
            target: Target::Window(id),
            cursor: false,
        })
    })?;

    let Reply::Screenshot(shot) = reply else {
        anyhow::bail!("expected a screenshot, got {reply:?}");
    };
    let image = decode(&shot.png()?)?;
    // 400×300 logical on an output at scale 2.
    assert_eq!((image.width, image.height), (800, 600));
    assert_eq!(image.pixel(400, 300), [0x33, 0x99, 0xff, 0xff]);
    Ok(())
}

#[test]
fn unknown_outputs_and_unknown_requests_get_clear_errors() -> anyhow::Result<()> {
    let mut compositor = compositor(Permission::Observe)?;

    let missing = with_client(&mut compositor, |client| {
        Ok(client.request(Request::Screenshot {
            target: Target::Output("NOPE".into()),
            cursor: false,
        }))
    })?;

    let Err(ClientError::Api(error)) = missing else {
        anyhow::bail!("expected an error, got {missing:?}");
    };
    assert_eq!(error.code, ErrorCode::NotFound);

    let socket = compositor.api_socket().to_path_buf();
    let raw = std::thread::spawn(move || -> std::io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.write_all(b"{\"id\":7,\"request\":\"fly\"}\n")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        Ok(line)
    });
    compositor.dispatch_until(TIMEOUT, |_| raw.is_finished())?;
    let line = raw
        .join()
        .map_err(|_| anyhow::anyhow!("the client thread panicked"))??;
    assert!(
        line.starts_with(r#"{"id":7,"error":{"code":"bad_request","#),
        "{line}"
    );
    Ok(())
}

struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

fn decode(png: &[u8]) -> anyhow::Result<Image> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size().unwrap_or_default()];
    let info = reader.next_frame(&mut rgba)?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba,
        "not RGBA: {:?}",
        info.color_type
    );
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}
