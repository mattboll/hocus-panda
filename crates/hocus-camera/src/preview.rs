//! The camera's window's preview (the shell's): a reader of the
//! Hocuspanda source, as an application in a call reads it, so that it
//! shows what the others see. On a thread of its own with PipeWire's
//! loop, linked to the source by itself; each frame made small (each of
//! its pixels the mean of the source's under it) and handed over, RGBA.
//!
//! While it reads, the source reads the camera: the camera is on while
//! the window shows its preview.

use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
};

use hocus_link::camera::{CAMERA_NODE, CAMERA_PREVIEW_NODE};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use crate::pods;

/// A frame of the preview: its size, RGBA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub size: (u32, u32),
    pub rgba: Vec<u8>,
}

/// The preview's thread, as its owner holds it: it ends when dropped.
pub struct Preview {
    quit: pw::channel::Sender<()>,
}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Preview")
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.quit.send(());
    }
}

/// The ports the registry announced, and the link once made.
#[derive(Default)]
struct Ports {
    source: Option<u32>,
    reader: Option<u32>,
    outputs: Vec<(u32, u32)>,
    inputs: Vec<(u32, u32)>,
    link: Option<pw::link::Link>,
}

impl Ports {
    fn pair(&self) -> Option<(u32, u32, u32, u32)> {
        let (source, reader) = (self.source?, self.reader?);
        let output = self.outputs.iter().find(|(n, _)| *n == source)?.1;
        let input = self.inputs.iter().find(|(n, _)| *n == reader)?.1;
        Some((source, output, reader, input))
    }
}

/// Starts reading the source on PipeWire's socket `remote` (else its
/// default), each frame made `size` large and handed to `each`.
pub fn start(
    remote: Option<PathBuf>,
    size: (u32, u32),
    each: impl Fn(Frame) + Send + 'static,
) -> Option<Preview> {
    start_as(remote, size, (CAMERA_PREVIEW_NODE, "Hocuspanda"), each)
}

/// The same as another reader: its node's name and its application's
/// (the tests' call, which the in-use sign counts).
pub fn start_as(
    remote: Option<PathBuf>,
    size: (u32, u32),
    (node, app): (&'static str, &'static str),
    each: impl Fn(Frame) + Send + 'static,
) -> Option<Preview> {
    let (quit, quitting) = pw::channel::channel::<()>();
    let started = std::thread::Builder::new()
        .name("camera preview".to_owned())
        .spawn(move || {
            if let Err(err) = run(remote, size, (node, app), quitting, each) {
                tracing::warn!(%err, "the camera's preview cannot read");
            }
        });
    match started {
        Ok(_) => Some(Preview { quit }),
        Err(err) => {
            tracing::warn!(%err, "the camera's preview's thread");
            None
        }
    }
}

fn run(
    remote: Option<PathBuf>,
    size: (u32, u32),
    (node, app): (&'static str, &'static str),
    quitting: pw::channel::Receiver<()>,
    each: impl Fn(Frame) + Send + 'static,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let props = remote.map(|r| {
        properties! {
            *pw::keys::REMOTE_NAME => r.to_string_lossy().into_owned(),
        }
    });
    let core = context.connect_rc(props)?;
    let stream = pw::stream::StreamRc::new(
        core.clone(),
        node,
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Camera",
            *pw::keys::NODE_NAME => node,
            *pw::keys::APP_NAME => app,
            "node.autoconnect" => "false",
        },
    )?;
    let ports = Rc::new(RefCell::new(Ports::default()));
    let registry = core.get_registry_rc()?;
    let linking = ports.clone();
    let link_core = core.clone();
    let _registry = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props else {
                return;
            };
            let mut ports = linking.borrow_mut();
            match global.type_ {
                pw::types::ObjectType::Node => match props.get("node.name") {
                    Some(CAMERA_NODE) => ports.source = Some(global.id),
                    Some(name) if name == node => ports.reader = Some(global.id),
                    _ => {}
                },
                pw::types::ObjectType::Port => {
                    let Some(node) = props.get("node.id").and_then(|n| n.parse().ok()) else {
                        return;
                    };
                    match props.get("port.direction") {
                        Some("out") => ports.outputs.push((node, global.id)),
                        Some("in") => ports.inputs.push((node, global.id)),
                        _ => {}
                    }
                }
                _ => {}
            }
            if ports.link.is_some() {
                return;
            }
            let Some((source, output, reader, input)) = ports.pair() else {
                return;
            };
            match link_core.create_object::<pw::link::Link>(
                "link-factory",
                &properties! {
                    *pw::keys::LINK_OUTPUT_NODE => source.to_string(),
                    *pw::keys::LINK_OUTPUT_PORT => output.to_string(),
                    *pw::keys::LINK_INPUT_NODE => reader.to_string(),
                    *pw::keys::LINK_INPUT_PORT => input.to_string(),
                },
            ) {
                Ok(link) => ports.link = Some(link),
                Err(err) => tracing::warn!(%err, "the preview could not be linked"),
            }
        })
        .register();
    let agreed = Arc::new(Mutex::new((0u32, 0u32)));
    let seen = agreed.clone();
    let mut small = Vec::new();
    let _listener = stream
        .add_local_listener::<()>()
        .param_changed(move |_, _, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::default();
            if info.parse(param).is_ok() {
                *seen.lock().unwrap_or_else(PoisonError::into_inner) =
                    (info.size().width, info.size().height);
            }
        })
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let from = *agreed.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let stride = usize::try_from(data.chunk().stride()).unwrap_or(0);
            let Some(bytes) = data.data() else {
                return;
            };
            if shrink(bytes, from, stride, size, &mut small) {
                each(Frame {
                    size,
                    rgba: small.clone(),
                });
            }
        })
        .register()?;
    let pod = pods::preview_format();
    let pod = Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::MAP_BUFFERS,
        &mut [pod],
    )?;
    let weak = mainloop.downgrade();
    let _quit = quitting.attach(mainloop.loop_(), move |()| {
        if let Some(mainloop) = weak.upgrade() {
            mainloop.quit();
        }
    });
    tracing::debug!("the camera's preview reads");
    mainloop.run();
    let _ = stream.disconnect();
    drop(ports);
    tracing::debug!("the camera's preview ends");
    Ok(())
}

/// B, G, R, x `bytes` (`from` large, rows of `stride`) made `to` large into
/// RGBA `out`, each pixel the mean of those under it: whether it was.
pub fn shrink(
    bytes: &[u8],
    from: (u32, u32),
    stride: usize,
    to: (u32, u32),
    out: &mut Vec<u8>,
) -> bool {
    let (fw, fh) = (from.0 as usize, from.1 as usize);
    let (tw, th) = (to.0 as usize, to.1 as usize);
    if fw == 0 || fh == 0 || tw == 0 || th == 0 {
        return false;
    }
    let stride = if stride == 0 { fw * 4 } else { stride };
    if bytes.len() < stride * (fh - 1) + fw * 4 {
        return false;
    }
    out.clear();
    out.reserve(tw * th * 4);
    for y in 0..th {
        let (y0, y1) = (y * fh / th, ((y + 1) * fh / th).max(y * fh / th + 1));
        for x in 0..tw {
            let (x0, x1) = (x * fw / tw, ((x + 1) * fw / tw).max(x * fw / tw + 1));
            let mut sum = [0u32; 3];
            let mut n = 0u32;
            for sy in y0..y1.min(fh) {
                for sx in x0..x1.min(fw) {
                    if let Some(&[b, g, r, _]) =
                        bytes.get(sy * stride + sx * 4..sy * stride + sx * 4 + 4)
                    {
                        sum = [
                            sum[0] + u32::from(r),
                            sum[1] + u32::from(g),
                            sum[2] + u32::from(b),
                        ];
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            out.extend_from_slice(&[
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                255,
            ]);
        }
    }
    true
}
