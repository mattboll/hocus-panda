//! The source on PipeWire, on PipeWire's loop (this program's main
//! thread).
//!
//! - **The source** ("Hocuspanda", `Video/Source`, `media.role = Camera`):
//!   always on the graph, a driver (it sends a frame as the camera gives
//!   one). Its stream streams while a reader is linked to it, and only
//!   then.
//! - **The real camera** is read while the source streams, and only then:
//!   a stream of this program's, linked by the program itself to the
//!   camera's node (no session manager links it: it must never read its
//!   own source). The camera: PipeWire's video sources that are not this
//!   one nor a screen cast, an infrared one last. It goes when the source
//!   stops streaming, and with it the GPU's work: nothing runs while
//!   nothing reads. Without a camera, a dark frame twice a second, with
//!   the banner.
//! - **Each frame** the camera sends: read into RGBA, its statistics (the
//!   face's light, Lumière's correction), made ([`crate::look`], on the
//!   GPU or the CPU), and handed to the source's free buffer, in the format
//!   its readers agreed to, and to the v4l2loopback device labelled
//!   "Hocuspanda" when there is one ([`crate::loopback`]).
//! - **Read** means the source streams or the device has a reader: the
//!   real camera is read while either is. While neither is, the device
//!   gets the still (the dark frame, with the banner). Without the device
//!   (looked for again every ten seconds, from sysfs alone), PipeWire
//!   only.
//!
//! What the streams and the registry say is handled one thing at a time
//! ([`Shared::dispatch`]): a call into PipeWire that says something at
//! once (a stream disconnected tells its state) queues it behind the
//! thing being handled.

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    rc::{Rc, Weak},
    time::Duration,
};

use hocus_link::camera::{
    CAMERA_DESCRIPTION, CAMERA_INPUT_NODE, CAMERA_NODE, CAMERA_PREFIX, CameraLook, FromCamera,
};
use pipewire as pw;
use pw::{properties::properties, spa, stream::StreamState};
use spa::pod::Pod;

use crate::{
    banner,
    convert::{self, Converter, Input, Output},
    gpu::{Gpu, Wanted},
    look::{self, BYTES, Correction, Cpu, FOLLOW, H, Making, Overlay, W},
    loopback, pods,
};

/// What the compositor asks.
#[derive(Debug)]
pub enum Ask {
    Look(CameraLook),
    /// Its line ended: so does the program.
    Quit,
}

/// How the program runs.
#[derive(Debug, Clone)]
pub struct Options {
    /// PipeWire's socket; else its default.
    pub remote: Option<PathBuf>,
    pub wanted: Wanted,
    /// The name the banner says when the compositor gives none: the
    /// account's.
    pub name: String,
}

/// The face's light follows this much of each frame: about a second.
const FACE_FOLLOW: f32 = 1.0 / 30.0;
/// It is said every this many frames, when it moved more than
/// [`FACE_STEP`].
const FACE_EVERY: u64 = 15;
const FACE_STEP: f32 = 0.004;
/// The dark frame's colour without a camera, the preview's own.
const NO_CAMERA: [u8; 3] = [0x1A, 0x1F, 0x2C];
/// Without the v4l2loopback device, it is looked for again every this
/// many ticks (ten seconds).
const LOOPBACK_AGAIN: u32 = 20;

#[derive(Debug)]
enum Ev {
    Ask(Ask),
    OutState(StreamState),
    OutFormat(Output),
    InState(StreamState),
    InFormat(Input),
    Node {
        id: u32,
        node: Node,
    },
    Port {
        id: u32,
        port: Port,
    },
    Removed(u32),
    /// Whether the v4l2loopback device has a reader.
    LoopbackRead(bool),
    Tick,
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    class: String,
    role: String,
    description: String,
    /// A V4L2 device's label, as PipeWire's node of it says it.
    card: String,
}

impl Node {
    /// A real camera: a video source that is not ours nor a screen cast,
    /// nor PipeWire's node of the v4l2loopback device it writes.
    fn camera(&self) -> bool {
        self.class == "Video/Source"
            && !self.name.starts_with(CAMERA_PREFIX)
            && self.role != "Screen"
            && self.card != loopback::LABEL
            && self.description != CAMERA_DESCRIPTION
    }

    /// An infrared one, last.
    fn infrared(&self) -> bool {
        let d = format!(" {} ", self.description.to_lowercase());
        d.contains(" ir ") || d.contains("infrared") || d.contains("infrarouge")
    }
}

#[derive(Debug, Clone, Copy)]
struct Port {
    node: u32,
    output: bool,
}

struct Out {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
    format: Option<Output>,
}

struct In {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
    camera: u32,
    node: Option<u32>,
    input: Option<Input>,
    link: Option<pw::link::Link>,
}

/// Where the frames are made.
enum Maker {
    Gpu(Box<Gpu>, Cpu),
    Cpu(Cpu),
}

impl Maker {
    fn make(&mut self, frame: &[u8], making: &Making<'_>, out: &mut [u8]) {
        match self {
            Self::Gpu(gpu, cpu) => {
                if !gpu.make(frame, making, out) {
                    tracing::warn!("the GPU failed: this frame made on the CPU");
                    cpu.make(frame, making, out);
                }
            }
            Self::Cpu(cpu) => cpu.make(frame, making, out),
        }
    }
}

struct State {
    core: pw::core::CoreRc,
    mainloop: pw::main_loop::MainLoopWeak,
    weak: Weak<Shared>,
    wanted: Wanted,
    name: String,
    tell: Box<dyn Fn(FromCamera)>,
    out: Option<Out>,
    input: Option<In>,
    nodes: BTreeMap<u32, Node>,
    ports: BTreeMap<u32, Port>,
    look: CameraLook,
    overlay: Option<Overlay>,
    maker: Option<Maker>,
    correction: Option<Correction>,
    face: Option<f32>,
    face_told: Option<f32>,
    frames: u64,
    /// The last frame of the camera's could not be read (said once).
    unread: bool,
    /// Read: the source streams, or the device has a reader.
    read: bool,
    source_read: bool,
    loopback_read: bool,
    /// The v4l2loopback device written, when there is one.
    loopback: Option<loopback::Writer>,
    /// What its thread tells the loop.
    loopback_tell: pw::channel::Sender<bool>,
    /// Its absence (or why it cannot be written) said once.
    loopback_said: bool,
    ticks: u32,
    rgba: Vec<u8>,
    made: Vec<u8>,
    converter: Converter,
}

/// The state and what waits to be handled.
struct Shared {
    state: RefCell<State>,
    pending: RefCell<VecDeque<Ev>>,
}

impl Shared {
    /// `ev` handled, after what waits; or queued, while something is.
    fn dispatch(&self, ev: Ev) {
        self.pending.borrow_mut().push_back(ev);
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return;
        };
        loop {
            let next = self.pending.borrow_mut().pop_front();
            let Some(ev) = next else {
                break;
            };
            state.handle(ev);
        }
    }
}

/// Runs the source until the compositor's line ends.
pub fn run(
    options: Options,
    asks: pw::channel::Receiver<Ask>,
    tell: Box<dyn Fn(FromCamera)>,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let props = options.remote.map(|r| {
        properties! {
            *pw::keys::REMOTE_NAME => r.to_string_lossy().into_owned(),
        }
    });
    let core = context.connect_rc(props)?;
    let registry = core.get_registry_rc()?;
    let (loopback_tell, loopback_heard) = pw::channel::channel::<bool>();
    let shared = Rc::new_cyclic(|weak: &Weak<Shared>| Shared {
        state: RefCell::new(State {
            core: core.clone(),
            mainloop: mainloop.downgrade(),
            weak: weak.clone(),
            wanted: options.wanted,
            name: options.name,
            tell,
            out: None,
            input: None,
            nodes: BTreeMap::new(),
            ports: BTreeMap::new(),
            look: CameraLook::default(),
            overlay: None,
            maker: None,
            correction: None,
            face: None,
            face_told: None,
            frames: 0,
            unread: false,
            read: false,
            source_read: false,
            loopback_read: false,
            loopback: None,
            loopback_tell,
            loopback_said: false,
            ticks: 0,
            rgba: vec![0; BYTES],
            made: vec![0; BYTES],
            converter: Converter::default(),
        }),
        pending: RefCell::new(VecDeque::new()),
    });
    let on_global = Rc::downgrade(&shared);
    let on_remove = Rc::downgrade(&shared);
    let _registry = registry
        .add_listener_local()
        .global(move |global| {
            let Some(shared) = on_global.upgrade() else {
                return;
            };
            let Some(props) = global.props else {
                return;
            };
            let get = |key: &str| props.get(key).unwrap_or_default().to_owned();
            match global.type_ {
                pw::types::ObjectType::Node => shared.dispatch(Ev::Node {
                    id: global.id,
                    node: Node {
                        name: get("node.name"),
                        class: get("media.class"),
                        role: get("media.role"),
                        description: get("node.description"),
                        card: get("api.v4l2.cap.card"),
                    },
                }),
                pw::types::ObjectType::Port => {
                    let Some(node) = props.get("node.id").and_then(|n| n.parse().ok()) else {
                        return;
                    };
                    shared.dispatch(Ev::Port {
                        id: global.id,
                        port: Port {
                            node,
                            output: props.get("port.direction") == Some("out"),
                        },
                    });
                }
                _ => {}
            }
        })
        .global_remove(move |id| {
            if let Some(shared) = on_remove.upgrade() {
                shared.dispatch(Ev::Removed(id));
            }
        })
        .register();
    let on_ask = Rc::downgrade(&shared);
    let _asks = asks.attach(mainloop.loop_(), move |ask| {
        if let Some(shared) = on_ask.upgrade() {
            shared.dispatch(Ev::Ask(ask));
        }
    });
    let on_loopback = Rc::downgrade(&shared);
    let _loopback = loopback_heard.attach(mainloop.loop_(), move |read| {
        if let Some(shared) = on_loopback.upgrade() {
            shared.dispatch(Ev::LoopbackRead(read));
        }
    });
    let on_tick = Rc::downgrade(&shared);
    let timer = mainloop.loop_().add_timer(move |_| {
        if let Some(shared) = on_tick.upgrade() {
            shared.dispatch(Ev::Tick);
        }
    });
    let half = Duration::from_millis(500);
    let _ = timer.update_timer(Some(half), Some(half));
    shared.state.borrow_mut().publish()?;
    shared.state.borrow_mut().open_loopback();
    mainloop.run();
    // The streams end before the loop and the core they were made on.
    if let Ok(mut state) = shared.state.try_borrow_mut() {
        state.stop_reading();
        state.out = None;
        state.loopback = None;
    }
    Ok(())
}

impl State {
    fn tell(&self, said: FromCamera) {
        (self.tell)(said);
    }

    fn dispatcher(&self) -> impl Fn(Ev) + 'static {
        let weak = self.weak.clone();
        move |ev| {
            if let Some(shared) = weak.upgrade() {
                shared.dispatch(ev);
            }
        }
    }

    /// The source on the graph.
    fn publish(&mut self) -> Result<(), pw::Error> {
        let stream = pw::stream::StreamRc::new(
            self.core.clone(),
            CAMERA_DESCRIPTION,
            properties! {
                *pw::keys::MEDIA_CLASS => "Video/Source",
                *pw::keys::MEDIA_ROLE => "Camera",
                *pw::keys::NODE_NAME => CAMERA_NODE,
                *pw::keys::NODE_DESCRIPTION => CAMERA_DESCRIPTION,
                *pw::keys::APP_NAME => "Hocuspanda",
            },
        )?;
        let states = self.dispatcher();
        let formats = self.dispatcher();
        let listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, old, new| {
                tracing::debug!(?old, ?new, "the source");
                states(Ev::OutState(new));
            })
            .param_changed(move |stream, _, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Some(output) = pods::output_of(param) else {
                    return;
                };
                let (stride, size) = output.layout();
                let pod = pods::buffers(stride, size);
                if let Some(pod) = Pod::from_bytes(&pod) {
                    let mut params = [pod];
                    if let Err(err) = stream.update_params(&mut params) {
                        tracing::warn!(%err, "the source's buffers not asked");
                    }
                }
                tracing::info!(?output, "a reader of the source agreed");
                formats(Ev::OutFormat(output));
            })
            .register()?;
        let pod = pods::source_format();
        let pod = Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
        stream.connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::DRIVER | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut [pod],
        )?;
        self.out = Some(Out {
            stream,
            _listener: listener,
            format: None,
        });
        Ok(())
    }

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Ask(Ask::Look(mut look)) => {
                if let Some(banner) = &mut look.banner
                    && banner.name.is_empty()
                {
                    banner.name.clone_from(&self.name);
                }
                if look.banner != self.look.banner {
                    self.overlay = look.banner.as_ref().map(banner::overlay);
                }
                tracing::info!(
                    lighting = look.lighting,
                    banner = look.banner.is_some(),
                    "the camera's look"
                );
                let changed = look.banner != self.look.banner;
                self.look = look;
                if changed && !self.read {
                    self.still();
                }
            }
            Ev::Ask(Ask::Quit) => {
                if let Some(mainloop) = self.mainloop.upgrade() {
                    mainloop.quit();
                }
            }
            Ev::OutState(new) => {
                self.source_read = match new {
                    StreamState::Paused => {
                        if let Some(out) = &self.out {
                            self.tell(FromCamera::Ready {
                                node: out.stream.node_id(),
                            });
                        }
                        false
                    }
                    StreamState::Streaming => true,
                    StreamState::Error(why) => {
                        tracing::warn!(why, "the source failed");
                        false
                    }
                    _ => false,
                };
                self.reading();
            }
            Ev::LoopbackRead(read) => {
                self.loopback_read = read;
                self.reading();
            }
            Ev::OutFormat(output) => {
                if let Some(out) = &mut self.out {
                    out.format = Some(output);
                }
            }
            Ev::InState(new) => {
                let node = self.input.as_ref().map(|i| i.stream.node_id());
                if let (StreamState::Paused | StreamState::Streaming, Some(input)) =
                    (&new, &mut self.input)
                {
                    input.node = node;
                }
                if let StreamState::Error(why) = &new {
                    tracing::warn!(why, "the camera's stream failed");
                }
                self.link();
            }
            Ev::InFormat(input) => {
                tracing::info!(?input, "the camera agreed");
                if let Some(i) = &mut self.input {
                    i.input = Some(input);
                }
            }
            Ev::Node { id, node } => {
                let camera = node.camera();
                self.nodes.insert(id, node);
                if camera && self.read && self.input.is_none() {
                    self.start_reading();
                }
            }
            Ev::Port { id, port } => {
                self.ports.insert(id, port);
                self.link();
            }
            Ev::Removed(id) => {
                self.nodes.remove(&id);
                self.ports.remove(&id);
                if self.input.as_ref().is_some_and(|i| i.camera == id) {
                    tracing::info!(id, "the camera went");
                    self.stop_input();
                    if self.read {
                        self.start_reading();
                    }
                }
            }
            Ev::Tick => {
                if self.read && self.input.is_none() {
                    self.no_camera_frame();
                }
                self.ticks = self.ticks.wrapping_add(1);
                if self.loopback.is_none() && self.ticks.is_multiple_of(LOOPBACK_AGAIN) {
                    self.open_loopback();
                }
            }
        }
    }

    /// Read or not, as the source streams or the device has a reader.
    fn reading(&mut self) {
        let read = self.source_read || self.loopback_read;
        if read == self.read {
            return;
        }
        self.read = read;
        tracing::info!(
            read,
            source = self.source_read,
            v4l2 = self.loopback_read,
            "the source"
        );
        self.tell(FromCamera::Read(read));
        if read {
            self.start_reading();
        } else {
            self.stop_reading();
            self.still();
        }
    }

    /// The v4l2loopback device, if there is one now: written from now on.
    fn open_loopback(&mut self) {
        let tell = self.loopback_tell.clone();
        match loopback::start(move |read| {
            let _ = tell.send(read);
        }) {
            Ok(Some(writer)) => {
                self.loopback = Some(writer);
                self.loopback_said = false;
                if !self.read {
                    self.still();
                }
            }
            Ok(None) => {
                if !self.loopback_said {
                    tracing::info!(
                        "no v4l2loopback device labelled Hocuspanda: the camera is on PipeWire only"
                    );
                    self.loopback_said = true;
                }
            }
            Err(err) => {
                if !self.loopback_said {
                    tracing::warn!(%err, "the v4l2loopback device cannot be written: the camera is on PipeWire only");
                    self.loopback_said = true;
                }
            }
        }
    }

    /// Nothing reads: the device's still, the dark frame with the banner.
    fn still(&mut self) {
        if self.loopback.is_some() {
            self.no_camera_frame();
        }
    }

    /// The camera to read: not an infrared one when another is there,
    /// else the first.
    fn choose(&self) -> Option<u32> {
        let cameras = || self.nodes.iter().filter(|(_, n)| n.camera());
        cameras()
            .find(|(_, n)| !n.infrared())
            .or_else(|| cameras().next())
            .map(|(id, _)| *id)
    }

    fn start_reading(&mut self) {
        if self.maker.is_none() {
            let maker = match Gpu::new(self.wanted) {
                Some(gpu) => {
                    self.tell(FromCamera::Device(Some(gpu.name().to_owned())));
                    Maker::Gpu(Box::new(gpu), Cpu::default())
                }
                None => {
                    self.tell(FromCamera::Device(None));
                    Maker::Cpu(Cpu::default())
                }
            };
            self.maker = Some(maker);
        }
        if self.input.is_some() {
            return;
        }
        let Some(camera) = self.choose() else {
            tracing::info!("no camera to read");
            self.tell(FromCamera::Source(None));
            self.no_camera_frame();
            return;
        };
        let description = self
            .nodes
            .get(&camera)
            .map(|n| n.description.clone())
            .unwrap_or_default();
        match self.read_camera(camera) {
            Ok(input) => {
                tracing::info!(camera, description, "the camera is read");
                self.tell(FromCamera::Source(Some(description)));
                self.input = Some(input);
            }
            Err(err) => tracing::warn!(%err, camera, "the camera cannot be read"),
        }
    }

    fn read_camera(&self, camera: u32) -> Result<In, pw::Error> {
        let stream = pw::stream::StreamRc::new(
            self.core.clone(),
            CAMERA_INPUT_NODE,
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Camera",
                *pw::keys::NODE_NAME => CAMERA_INPUT_NODE,
                *pw::keys::APP_NAME => "Hocuspanda",
                "node.autoconnect" => "false",
            },
        )?;
        let states = self.dispatcher();
        let formats = self.dispatcher();
        let weak = self.weak.clone();
        let listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, old, new| {
                tracing::debug!(?old, ?new, "the camera's stream");
                states(Ev::InState(new));
            })
            .param_changed(move |stream, _, id, param| {
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                if let Some(input) = param.and_then(pods::input_of) {
                    let pod = pods::camera_buffers();
                    if let Some(pod) = Pod::from_bytes(&pod)
                        && let Err(err) = stream.update_params(&mut [pod])
                    {
                        tracing::warn!(%err, "the camera's buffers not asked");
                    }
                    formats(Ev::InFormat(input));
                }
            })
            .process(move |stream, _| {
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                let Ok(mut state) = shared.state.try_borrow_mut() else {
                    // Something is being handled: this frame is dropped.
                    let _ = stream.dequeue_buffer();
                    return;
                };
                state.frame(stream);
            })
            .register()?;
        let params = pods::camera_formats();
        let mut pods: Vec<&Pod> = params.iter().filter_map(|b| Pod::from_bytes(b)).collect();
        stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::MAP_BUFFERS,
            &mut pods,
        )?;
        Ok(In {
            stream,
            _listener: listener,
            camera,
            node: None,
            input: None,
            link: None,
        })
    }

    /// The camera's output port linked to the stream's input port, once
    /// both are on the graph.
    fn link(&mut self) {
        let Some(input) = &mut self.input else {
            return;
        };
        if input.link.is_some() {
            return;
        }
        let Some(node) = input.node.filter(|n| *n != 0 && *n != u32::MAX) else {
            return;
        };
        let find = |of: u32, output: bool| {
            self.ports
                .iter()
                .find(|(_, p)| p.node == of && p.output == output)
                .map(|(id, _)| *id)
        };
        let (Some(from), Some(to)) = (find(input.camera, true), find(node, false)) else {
            return;
        };
        match self.core.create_object::<pw::link::Link>(
            "link-factory",
            &properties! {
                *pw::keys::LINK_OUTPUT_NODE => input.camera.to_string(),
                *pw::keys::LINK_OUTPUT_PORT => from.to_string(),
                *pw::keys::LINK_INPUT_NODE => node.to_string(),
                *pw::keys::LINK_INPUT_PORT => to.to_string(),
            },
        ) {
            Ok(link) => {
                tracing::debug!(camera = input.camera, node, "linked to the camera");
                input.link = Some(link);
            }
            Err(err) => tracing::warn!(%err, "the camera could not be linked"),
        }
    }

    fn stop_input(&mut self) {
        if let Some(input) = self.input.take() {
            drop(input.link);
            let _ = input.stream.disconnect();
            tracing::info!(camera = input.camera, "the camera is no longer read");
        }
    }

    /// Nothing reads: the camera and the GPU let go.
    fn stop_reading(&mut self) {
        self.stop_input();
        self.maker = None;
        self.correction = None;
        self.face = None;
        self.face_told = None;
    }

    /// A frame from the camera's stream, made and handed to the source.
    fn frame(&mut self, stream: &pw::stream::Stream) {
        let Some(input) = self.input.as_ref().and_then(|i| i.input) else {
            let _ = stream.dequeue_buffer();
            return;
        };
        let read = 'read: {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                break 'read Err("a buffer without data");
            };
            let (offset, size, stride) = {
                let chunk = data.chunk();
                (
                    chunk.offset() as usize,
                    chunk.size() as usize,
                    u32::try_from(chunk.stride()).unwrap_or(0),
                )
            };
            let kind = data.type_();
            let Some(bytes) = data.data() else {
                tracing::debug!(?kind, "a frame not mapped");
                break 'read Err("a frame not in this process's memory");
            };
            let end = if size == 0 {
                bytes.len()
            } else {
                (offset + size).min(bytes.len())
            };
            let Some(bytes) = bytes.get(offset.min(end)..end) else {
                break 'read Err("a frame's chunk outside its buffer");
            };
            let input = Input { stride, ..input };
            if self.converter.to_rgba(&input, bytes, &mut self.rgba) {
                Ok(())
            } else {
                Err("a frame that cannot be decoded")
            }
        };
        if let Err(why) = read {
            if !self.unread {
                tracing::warn!(
                    ?input,
                    why,
                    "the camera's frames cannot be read: the still is sent"
                );
                self.unread = true;
            }
            return;
        }
        self.unread = false;
        let stats = look::stats(&self.rgba, W, H);
        self.frames += 1;
        if self.frames == 1 {
            tracing::debug!(?stats, "the camera's first frame");
        }
        let face = self
            .face
            .map_or(stats.face, |f| f + (stats.face - f) * FACE_FOLLOW);
        self.face = Some(face);
        if self.frames.is_multiple_of(FACE_EVERY)
            && self.face_told.is_none_or(|t| (t - face).abs() > FACE_STEP)
        {
            self.face_told = Some(face);
            self.tell(FromCamera::Face(face));
        }
        let wanted = Correction::of(&stats);
        let correction = self
            .correction
            .map_or(wanted, |c| c.towards(wanted, FOLLOW));
        self.correction = Some(correction);
        let making = Making {
            lighting: self.look.lighting,
            correction,
            overlay: self.overlay.as_ref(),
        };
        if let Some(maker) = &mut self.maker {
            maker.make(&self.rgba, &making, &mut self.made);
        }
        self.send();
    }

    /// Without a camera, a dark frame, with the banner.
    fn no_camera_frame(&mut self) {
        for px in self.rgba.as_chunks_mut::<4>().0.iter_mut() {
            let [r, g, b] = NO_CAMERA;
            px.copy_from_slice(&[r, g, b, 255]);
        }
        let making = Making {
            lighting: false,
            correction: Correction::NONE,
            overlay: self.overlay.as_ref(),
        };
        Cpu::default().make(&self.rgba, &making, &mut self.made);
        self.send();
    }

    /// The made frame to the v4l2loopback device, and into the source's
    /// free buffer, if a reader is ready.
    fn send(&mut self) {
        if let Some(loopback) = &self.loopback {
            loopback.send(&self.made);
        }
        let Some(out) = &self.out else {
            return;
        };
        if out.stream.state() != StreamState::Streaming {
            return;
        }
        let format = out.format.unwrap_or(Output::Bgrx);
        let Some(mut buffer) = out.stream.dequeue_buffer() else {
            tracing::debug!("no free buffer: a frame dropped");
            return;
        };
        if let Some(data) = buffer.datas_mut().first_mut() {
            let written = data
                .data()
                .map_or(0, |memory| convert::write(&self.made, format, memory));
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = i32::try_from(format.layout().0).unwrap_or(i32::MAX);
            *chunk.size_mut() = u32::try_from(written).unwrap_or(0);
        }
        drop(buffer);
        if out.stream.is_driving() {
            let _ = out.stream.trigger_process();
        }
    }
}
