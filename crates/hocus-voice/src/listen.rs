//! A reader of the Hocuspanda source, as an application in a call reads
//! it: the camera and microphone's window's level meter (the shell's),
//! and the tests' call. On a thread of its own with PipeWire's loop,
//! linked to the source by itself; each quantum's samples handed over.
//!
//! While it reads, the source reads the microphone: the microphone is on
//! while the window shows its meter.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use hocus_link::voice::{VOICE_METER_NODE, VOICE_NODE};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use crate::serve;

/// The reader's thread, as its owner holds it: it ends when dropped.
pub struct Listener {
    quit: pw::channel::Sender<()>,
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Listener")
    }
}

impl Drop for Listener {
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
/// default), each quantum's samples (48 kHz, mono) handed to `each`.
pub fn start(
    remote: Option<PathBuf>,
    each: impl FnMut(&[f32]) + Send + 'static,
) -> Option<Listener> {
    start_as(remote, (VOICE_METER_NODE, "Hocuspanda"), each)
}

/// The same as another reader: its node's name and its application's
/// (the tests' call, which the in-use sign counts).
pub fn start_as(
    remote: Option<PathBuf>,
    (node, app): (&'static str, &'static str),
    each: impl FnMut(&[f32]) + Send + 'static,
) -> Option<Listener> {
    let (quit, quitting) = pw::channel::channel::<()>();
    let started = std::thread::Builder::new()
        .name("voice listener".to_owned())
        .spawn(move || {
            if let Err(err) = run(remote, (node, app), quitting, each) {
                tracing::warn!(%err, "the Hocuspanda microphone cannot be read");
            }
        });
    match started {
        Ok(_) => Some(Listener { quit }),
        Err(err) => {
            tracing::warn!(%err, "the voice listener's thread");
            None
        }
    }
}

fn run(
    remote: Option<PathBuf>,
    (node, app): (&'static str, &'static str),
    quitting: pw::channel::Receiver<()>,
    mut each: impl FnMut(&[f32]) + Send + 'static,
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
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::NODE_NAME => node,
            *pw::keys::APP_NAME => app,
            *pw::keys::NODE_LATENCY => "256/48000",
            "node.autoconnect" => "false",
            "adapter.auto-port-config" => "{ mode = dsp monitor = false position = preserve }",
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
                    Some(VOICE_NODE) => ports.source = Some(global.id),
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
                Err(err) => tracing::warn!(%err, "the listener could not be linked"),
            }
        })
        .register();
    let mut samples = Vec::with_capacity(2048);
    let _listener = stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            serve::samples_in(&mut buffer, &mut samples);
            each(&samples);
        })
        .register()?;
    let pod = serve::format_pod();
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
    tracing::debug!("the Hocuspanda microphone is listened to");
    mainloop.run();
    let _ = stream.disconnect();
    drop(ports);
    Ok(())
}
