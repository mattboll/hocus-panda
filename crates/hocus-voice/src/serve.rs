//! The source on PipeWire, on PipeWire's loop (this program's main
//! thread), its samples on PipeWire's real-time thread.
//!
//! - **The source** ("Hocuspanda", `Audio/Source`, 48 kHz, mono): always
//!   on the graph, never a driver: the real microphone's clock drives it,
//!   both its streams being of one `node.group`. It is read while a link
//!   goes out of its node (the registry's links): not by its stream's
//!   state, which the group keeps running while the microphone's stream
//!   runs. A low `priority.session`, so
//!   that a session manager never makes it the default microphone of
//!   itself; the same `node.link-group` as its reading of the microphone,
//!   so that one never links the other.
//! - **The real microphone** is read while the source is, and only
//!   then: a stream of this program's (`hocuspanda-voice-input`, not
//!   autoconnected), linked by the program itself to the microphone's
//!   first output port, and linked again whenever that link goes: a
//!   session manager makes a node's ports again when it sets them up
//!   (WirePlumber does, for the stream, some time after it appears), and
//!   the link goes with the ports it joined (#911). The microphone: the
//!   session's default source (the `default` metadata), unless it is this
//!   one; else the source of highest `priority.session`. Never one whose
//!   jack is empty while another can be read: its device's route says so
//!   (`available` no, as WirePlumber reads it to choose the default), and
//!   the choice is made again when a jack is plugged in or pulled out. It
//!   is said to the compositor as it changes, read or not (a meeting's
//!   transcription records it).
//! - **Each quantum** the microphone sends is made by the voice
//!   ([`crate::effect::Voicer`]) on the real-time thread, into a queue the
//!   source's own quantum takes from. The two run in the same cycle, in
//!   either order: when the source comes first, it takes what the last
//!   cycle made, and the queue keeps that one quantum from then on.
//!   Nothing else waits: the queue never holds more than 50 ms.
//! - **A change of voice** is made off the real-time thread (Sans bruit's
//!   models take some hundreds of milliseconds to read), then handed to
//!   it, which fades from one to the other.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    rc::{Rc, Weak},
    sync::{Arc, Mutex, PoisonError},
};

use hocus_link::voice::{
    FromVoice, Microphone, VOICE_DESCRIPTION, VOICE_INPUT_NODE, VOICE_NODE, VOICE_PREFIX, Voice,
    VoiceLook,
};
use pipewire as pw;
use pw::{properties::properties, proxy::ProxyT, spa, stream::StreamState};
use spa::pod::Pod;

use crate::{
    RATE,
    effect::{self, Effect, Voicer},
};

/// What the compositor asks.
#[derive(Debug)]
pub enum Ask {
    Look(VoiceLook),
    /// Its line ended: so does the program.
    Quit,
}

/// How the program runs.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// PipeWire's socket; else its default.
    pub remote: Option<PathBuf>,
}

/// The quantum the streams ask for: 256 samples, 5.3 ms.
const LATENCY: &str = "256/48000";
/// Both streams' group: one driver for both.
const GROUP: &str = "hocuspanda-voice";
/// The queue between the two never holds more than this: 50 ms.
const MOST_QUEUED: usize = RATE / 20;
/// A quantum's samples at most, in one go.
const CHUNK: usize = 1024;

enum Ev {
    Ask(Ask),
    OutState(StreamState),
    InState(StreamState),
    Node {
        id: u32,
        node: Node,
    },
    Port {
        id: u32,
        port: Port,
    },
    /// A link, from the node out of which it goes to the one into which,
    /// and between which ports.
    Link {
        id: u32,
        from: u32,
        to: u32,
        ports: (u32, u32),
    },
    Removed(u32),
    /// An error PipeWire said about one of this program's objects.
    Error {
        proxy: u32,
        message: String,
    },
    /// A microphone's place among its device's (`card.profile.device`),
    /// which its device's routes name.
    Place {
        node: u32,
        place: i32,
    },
    /// A route of a device, for the node at its `place`: whether something
    /// can be heard there (a jack's route is not while it is empty).
    Route {
        device: u32,
        place: i32,
        available: bool,
    },
    /// The `default` metadata's source: the chosen one or the one in use.
    Default {
        configured: bool,
        name: Option<String>,
    },
    /// A voice made, off the loop.
    Made(Voice, Box<dyn Effect>),
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    class: String,
    description: String,
    priority: i64,
    /// Its device (`device.id`): a sound card's, else none.
    device: Option<u32>,
}

impl Node {
    /// A microphone: a source that is not this one.
    fn microphone(&self) -> bool {
        matches!(self.class.as_str(), "Audio/Source" | "Audio/Source/Virtual")
            && !self.name.starts_with(VOICE_PREFIX)
            && self.description != VOICE_DESCRIPTION
    }
}

#[derive(Debug, Clone, Copy)]
struct Port {
    node: u32,
    /// Its place among its node's ports (`port.id`): the first channel's
    /// is 0, whatever global id it was given.
    index: u32,
    output: bool,
    monitor: bool,
}

/// A proxy kept to follow an object: a microphone's place, a device's
/// routes.
enum Watched {
    Node {
        _proxy: pw::node::Node,
        _listener: pw::node::NodeListener,
    },
    Device {
        _proxy: pw::device::Device,
        _listener: pw::device::DeviceListener,
    },
}

/// What both real-time callbacks share: the voice, and what it made that
/// the source has not taken yet.
#[derive(Debug, Default)]
pub struct Engine {
    voicer: Voicer,
    queue: VecDeque<f32>,
    block: Vec<f32>,
}

impl Engine {
    /// Samples from the microphone, made by the voice into the queue.
    pub fn heard(&mut self, samples: &[f32]) {
        self.block.clear();
        self.block.extend_from_slice(samples);
        self.voicer.run(&mut self.block);
        self.queue.extend(self.block.iter().copied());
        let extra = self.queue.len().saturating_sub(MOST_QUEUED);
        self.queue.drain(..extra);
    }

    /// The source's quantum: what was made, else silence.
    pub fn said(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            *o = self.queue.pop_front().unwrap_or(0.0);
        }
    }

    fn clear(&mut self) {
        self.voicer.reset();
        self.queue.clear();
    }
}

type Shared = Arc<Mutex<Engine>>;

fn lock(engine: &Shared) -> std::sync::MutexGuard<'_, Engine> {
    engine.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Out {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
}

struct In {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
    microphone: u32,
    node: Option<u32>,
    link: Option<MicLink>,
    /// The links asked while it reads: a link refused or undone each
    /// time it is made is not asked again without end.
    asked: u32,
}

/// The link from the microphone's port to the stream's, as asked.
struct MicLink {
    proxy: pw::link::Link,
    /// The microphone's port and the stream's.
    ports: (u32, u32),
    state: LinkState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkState {
    /// Asked, not yet on the graph.
    Asked,
    /// On the graph, this global.
    Made(u32),
}

/// The links asked for one reading of the microphone, at most: each
/// setting up of its ports or the stream's undoes one, and makes the next
/// one; a link undone as soon as it is made stops there.
const MOST_LINKS: u32 = 16;

struct State {
    core: pw::core::CoreRc,
    registry: pw::registry::RegistryRc,
    mainloop: pw::main_loop::MainLoopWeak,
    weak: Weak<Loop>,
    tell: Box<dyn Fn(FromVoice)>,
    made: pw::channel::Sender<(Voice, Box<dyn Effect>)>,
    engine: Shared,
    out: Option<Out>,
    input: Option<In>,
    nodes: BTreeMap<u32, Node>,
    ports: BTreeMap<u32, Port>,
    links: BTreeMap<u32, (u32, u32)>,
    /// The microphones and sound cards followed, by global id.
    watched: BTreeMap<u32, Watched>,
    /// Each microphone's place among its device's.
    places: BTreeMap<u32, i32>,
    /// The routes nothing can be heard on: a device and a place.
    empty: BTreeSet<(u32, i32)>,
    /// The source's node, once on the graph.
    node: Option<u32>,
    metadata: Option<(pw::metadata::Metadata, pw::metadata::MetadataListener)>,
    default: Option<String>,
    configured: Option<String>,
    /// The voice asked, and the one being made, if one is.
    voice: Voice,
    making: Option<Voice>,
    read: bool,
    told: Option<Option<Microphone>>,
}

/// The state and what waits to be handled.
struct Loop {
    state: RefCell<State>,
    pending: RefCell<VecDeque<Ev>>,
}

impl Loop {
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

    /// `global` followed: a node's place, a device's routes, the default
    /// microphone.
    fn watch(&self, global: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>) {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            tracing::warn!(
                id = global.id,
                "an object cannot be followed: the state is busy"
            );
            return;
        };
        match global.type_ {
            pw::types::ObjectType::Node => state.watch_node(global),
            pw::types::ObjectType::Device => state.watch_device(global),
            pw::types::ObjectType::Metadata => state.watch_defaults(global),
            _ => {}
        }
    }
}

/// Runs the source until the compositor's line ends.
pub fn run(
    options: Options,
    asks: pw::channel::Receiver<Ask>,
    tell: Box<dyn Fn(FromVoice)>,
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
    let (made, making) = pw::channel::channel::<(Voice, Box<dyn Effect>)>();
    let shared = Rc::new_cyclic(|weak: &Weak<Loop>| Loop {
        state: RefCell::new(State {
            core: core.clone(),
            registry: registry.clone(),
            mainloop: mainloop.downgrade(),
            weak: weak.clone(),
            tell,
            made,
            engine: Shared::default(),
            out: None,
            input: None,
            nodes: BTreeMap::new(),
            ports: BTreeMap::new(),
            links: BTreeMap::new(),
            watched: BTreeMap::new(),
            places: BTreeMap::new(),
            empty: BTreeSet::new(),
            node: None,
            metadata: None,
            default: None,
            configured: None,
            voice: Voice::AsItIs,
            making: None,
            read: false,
            told: None,
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
                pw::types::ObjectType::Node => {
                    let node = Node {
                        name: get("node.name"),
                        class: get("media.class"),
                        description: get("node.description"),
                        priority: get("priority.session").parse().unwrap_or(0),
                        device: props.get("device.id").and_then(|n| n.parse().ok()),
                    };
                    // A sound card's: its place, which the registry does
                    // not say, read from its own properties.
                    let follow = node.microphone() && node.device.is_some();
                    shared.dispatch(Ev::Node {
                        id: global.id,
                        node,
                    });
                    if follow {
                        shared.watch(global);
                    }
                }
                pw::types::ObjectType::Device
                    if props.get("media.class") == Some("Audio/Device") =>
                {
                    shared.watch(global);
                }
                pw::types::ObjectType::Port => {
                    let Some(node) = props.get("node.id").and_then(|n| n.parse().ok()) else {
                        return;
                    };
                    shared.dispatch(Ev::Port {
                        id: global.id,
                        port: Port {
                            node,
                            index: props
                                .get("port.id")
                                .and_then(|n| n.parse().ok())
                                .unwrap_or(u32::MAX),
                            output: props.get("port.direction") == Some("out"),
                            monitor: props.get("port.monitor") == Some("true"),
                        },
                    });
                }
                pw::types::ObjectType::Link => {
                    let number = |key: &str| props.get(key).and_then(|n| n.parse().ok());
                    if let (Some(from), Some(to)) =
                        (number("link.output.node"), number("link.input.node"))
                    {
                        let ports = (
                            number("link.output.port").unwrap_or(u32::MAX),
                            number("link.input.port").unwrap_or(u32::MAX),
                        );
                        shared.dispatch(Ev::Link {
                            id: global.id,
                            from,
                            to,
                            ports,
                        });
                    }
                }
                pw::types::ObjectType::Metadata
                    if props.get("metadata.name") == Some("default") =>
                {
                    shared.watch(global);
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
    // A link refused (a port gone meanwhile, formats that do not meet)
    // comes as an error on the core, never as the call's result.
    let on_error = Rc::downgrade(&shared);
    let _errors = core
        .add_listener_local()
        .error(move |proxy, _, res, message| {
            if let Some(shared) = on_error.upgrade() {
                shared.dispatch(Ev::Error {
                    proxy,
                    message: format!("{message} ({res})"),
                });
            }
        })
        .register();
    let on_ask = Rc::downgrade(&shared);
    let _asks = asks.attach(mainloop.loop_(), move |ask| {
        if let Some(shared) = on_ask.upgrade() {
            shared.dispatch(Ev::Ask(ask));
        }
    });
    let on_made = Rc::downgrade(&shared);
    let _made = making.attach(mainloop.loop_(), move |(voice, effect)| {
        if let Some(shared) = on_made.upgrade() {
            shared.dispatch(Ev::Made(voice, effect));
        }
    });
    shared.state.borrow_mut().publish()?;
    mainloop.run();
    // The streams end before the loop and the core they were made on.
    if let Ok(mut state) = shared.state.try_borrow_mut() {
        state.stop_reading();
        state.out = None;
        state.metadata = None;
    }
    Ok(())
}

/// Raw audio, 32-bit floats, 48 kHz, mono.
pub fn format_pod() -> Vec<u8> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE as u32);
    info.set_channels(1);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .unwrap_or_default()
}

/// A buffer's samples, read into `into`.
pub fn samples_in(buffer: &mut pw::buffer::Buffer<'_>, into: &mut Vec<f32>) {
    into.clear();
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let offset = data.chunk().offset() as usize;
    let size = data.chunk().size() as usize;
    if let Some(bytes) = data.data() {
        let end = (offset + size).min(bytes.len());
        let bytes = bytes.get(offset.min(end)..end).unwrap_or_default();
        into.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b)),
        );
    }
}

/// A buffer filled by `fill`, as many samples as asked (else as it holds,
/// a [`CHUNK`] at most).
pub fn samples_out(buffer: &mut pw::buffer::Buffer<'_>, mut fill: impl FnMut(&mut [f32])) {
    let requested = buffer.requested() as usize;
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let Some(bytes) = data.data() else {
        return;
    };
    let room = bytes.len() / 4;
    let n = if requested > 0 {
        requested.min(room)
    } else {
        room.min(CHUNK)
    };
    let mut made = [0f32; CHUNK];
    let mut written = 0;
    for chunk in bytes.as_chunks_mut::<4>().0.chunks_mut(CHUNK) {
        let k = chunk.len().min(n - written);
        if k == 0 {
            break;
        }
        let Some(made) = made.get_mut(..k) else {
            break;
        };
        fill(made);
        for (b, s) in chunk.iter_mut().zip(made.iter()) {
            *b = s.to_le_bytes();
        }
        written += k;
    }
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = 4;
    *chunk.size_mut() = (written * 4) as u32;
}

impl State {
    fn tell(&self, said: FromVoice) {
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

    /// The `default` metadata followed: the default source, and the one
    /// chosen.
    fn watch_defaults(&mut self, global: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>) {
        let metadata = match self.registry.bind::<pw::metadata::Metadata, _>(global) {
            Ok(metadata) => metadata,
            Err(err) => {
                tracing::warn!(%err, "the default microphone cannot be followed");
                return;
            }
        };
        let dispatch = self.dispatcher();
        let listener = metadata
            .add_listener_local()
            .property(move |subject, key, _, value| {
                if subject != 0 {
                    return 0;
                }
                let configured = match key {
                    Some("default.audio.source") => false,
                    Some("default.configured.audio.source") => true,
                    _ => return 0,
                };
                dispatch(Ev::Default {
                    configured,
                    name: value.and_then(name_of),
                });
                0
            })
            .register();
        self.metadata = Some((metadata, listener));
    }

    /// A microphone of a sound card followed: its place among its
    /// device's, which only its own properties say.
    fn watch_node(&mut self, global: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>) {
        let proxy = match self.registry.bind::<pw::node::Node, _>(global) {
            Ok(proxy) => proxy,
            Err(err) => {
                tracing::warn!(%err, id = global.id, "a microphone's place cannot be read");
                return;
            }
        };
        let dispatch = self.dispatcher();
        let node = global.id;
        let listener = proxy
            .add_listener_local()
            .info(move |info| {
                let place = info
                    .props()
                    .and_then(|p| p.get("card.profile.device"))
                    .and_then(|n| n.parse().ok());
                if let Some(place) = place {
                    dispatch(Ev::Place { node, place });
                }
            })
            .register();
        self.watched.insert(
            node,
            Watched::Node {
                _proxy: proxy,
                _listener: listener,
            },
        );
    }

    /// A sound card followed: its routes, each time they change (a jack
    /// plugged in or pulled out).
    fn watch_device(&mut self, global: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>) {
        let proxy = match self.registry.bind::<pw::device::Device, _>(global) {
            Ok(proxy) => proxy,
            Err(err) => {
                tracing::warn!(%err, id = global.id, "a sound card's routes cannot be read");
                return;
            }
        };
        let dispatch = self.dispatcher();
        let device = global.id;
        let listener = proxy
            .add_listener_local()
            .param(move |_, kind, _, _, param| {
                if kind != spa::param::ParamType::Route {
                    return;
                }
                if let Some((place, available)) = param.and_then(route_of) {
                    dispatch(Ev::Route {
                        device,
                        place,
                        available,
                    });
                }
            })
            .register();
        proxy.subscribe_params(&[spa::param::ParamType::Route]);
        self.watched.insert(
            device,
            Watched::Device {
                _proxy: proxy,
                _listener: listener,
            },
        );
    }

    /// The source on the graph.
    fn publish(&mut self) -> Result<(), pw::Error> {
        let stream = pw::stream::StreamRc::new(
            self.core.clone(),
            VOICE_DESCRIPTION,
            properties! {
                *pw::keys::MEDIA_CLASS => "Audio/Source",
                *pw::keys::NODE_NAME => VOICE_NODE,
                *pw::keys::NODE_DESCRIPTION => VOICE_DESCRIPTION,
                *pw::keys::APP_NAME => "Hocuspanda",
                *pw::keys::NODE_LATENCY => LATENCY,
                "node.group" => GROUP,
                "node.link-group" => GROUP,
                "priority.session" => "100",
                "device.icon-name" => "audio-input-microphone",
                "adapter.auto-port-config" => "{ mode = dsp monitor = false position = preserve }",
            },
        )?;
        let states = self.dispatcher();
        let engine = self.engine.clone();
        let listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, old, new| {
                tracing::debug!(?old, ?new, "the source");
                states(Ev::OutState(new));
            })
            .process(move |stream, _| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let mut engine = lock(&engine);
                samples_out(&mut buffer, |out| engine.said(out));
            })
            .register()?;
        let pod = format_pod();
        let pod = Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
        stream.connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
            &mut [pod],
        )?;
        self.out = Some(Out {
            stream,
            _listener: listener,
        });
        Ok(())
    }

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Ask(Ask::Look(look)) => {
                tracing::info!(voice = ?look.voice, "the voice asked");
                self.voice = look.voice;
                self.make();
            }
            Ev::Ask(Ask::Quit) => {
                if let Some(mainloop) = self.mainloop.upgrade() {
                    mainloop.quit();
                }
            }
            Ev::Made(voice, effect) => {
                self.making = None;
                if voice == self.voice {
                    tracing::info!(?voice, latency = effect.latency(), "the voice made");
                    {
                        let mut engine = lock(&self.engine);
                        if self.read {
                            engine.voicer.change(voice, effect);
                        } else {
                            engine.voicer = Voicer::new(voice, effect);
                        }
                    }
                    self.tell(FromVoice::Voice(voice));
                } else {
                    // Another was asked meanwhile.
                    self.make();
                }
            }
            Ev::OutState(new) => {
                match new {
                    StreamState::Paused | StreamState::Streaming if self.node.is_none() => {
                        if let Some(out) = &self.out {
                            let node = out.stream.node_id();
                            self.node = Some(node);
                            self.tell(FromVoice::Ready { node });
                        }
                    }
                    StreamState::Error(why) => {
                        tracing::warn!(why, "the source failed");
                    }
                    _ => {}
                }
                self.reading();
            }
            Ev::Link {
                id,
                from,
                to,
                ports,
            } => {
                self.links.insert(id, (from, to));
                if let Some(input) = &mut self.input
                    && let Some(link) = &mut input.link
                    && link.state == LinkState::Asked
                    && link.ports == ports
                    && from == input.microphone
                    && Some(to) == input.node
                {
                    link.state = LinkState::Made(id);
                    tracing::info!(
                        microphone = input.microphone,
                        node = to,
                        ports = ?ports,
                        "linked to the microphone"
                    );
                }
                self.reading();
            }
            Ev::InState(new) => {
                let node = self.input.as_ref().map(|i| i.stream.node_id());
                if let (StreamState::Paused | StreamState::Streaming, Some(input)) =
                    (&new, &mut self.input)
                {
                    input.node = node;
                }
                if let StreamState::Error(why) = &new {
                    tracing::warn!(why, "the microphone's stream failed");
                }
                self.link();
            }
            Ev::Node { id, node } => {
                self.nodes.insert(id, node);
                self.microphone_changed();
            }
            Ev::Port { id, port } => {
                self.ports.insert(id, port);
                self.link();
            }
            Ev::Removed(id) => {
                self.nodes.remove(&id);
                self.ports.remove(&id);
                self.watched.remove(&id);
                self.places.remove(&id);
                self.empty.retain(|(device, _)| *device != id);
                if self.links.remove(&id).is_some() {
                    self.reading();
                }
                if self.input.as_ref().is_some_and(|i| i.microphone == id) {
                    tracing::info!(id, "the microphone went");
                    self.stop_input();
                }
                if let Some(input) = &mut self.input
                    && input
                        .link
                        .as_ref()
                        .is_some_and(|l| l.state == LinkState::Made(id))
                {
                    tracing::info!(
                        microphone = input.microphone,
                        "the link to the microphone went"
                    );
                    input.link = None;
                }
                self.microphone_changed();
                // A port gone, or the link: linked again to the ports there are.
                self.link();
            }
            Ev::Error { proxy, message } => {
                let ours = self.input.as_mut().and_then(|input| {
                    let link = input.link.take_if(|l| l.proxy.upcast_ref().id() == proxy)?;
                    Some((input.microphone, link.ports))
                });
                match ours {
                    // Asked again when ports come or go.
                    Some((microphone, ports)) => tracing::warn!(
                        message,
                        microphone,
                        ports = ?ports,
                        "the microphone could not be linked"
                    ),
                    None => tracing::debug!(proxy, message, "PipeWire said an error"),
                }
            }
            Ev::Place { node, place } => {
                if self.places.insert(node, place) != Some(place) {
                    self.microphone_changed();
                }
            }
            Ev::Route {
                device,
                place,
                available,
            } => {
                let changed = if available {
                    self.empty.remove(&(device, place))
                } else {
                    self.empty.insert((device, place))
                };
                if changed {
                    tracing::info!(device, place, available, "a microphone's route");
                    self.microphone_changed();
                }
            }
            Ev::Default { configured, name } => {
                if configured {
                    self.configured = name;
                } else {
                    self.default = name;
                }
                self.microphone_changed();
            }
        }
    }

    /// The voice asked made, off the loop, unless it plays or one is being
    /// made.
    fn make(&mut self) {
        let playing = lock(&self.engine).voicer.voice();
        if playing == self.voice || self.making.is_some() {
            return;
        }
        let voice = self.voice;
        self.making = Some(voice);
        let made = self.made.clone();
        let started = std::thread::Builder::new()
            .name("voice's effect".to_owned())
            .spawn(move || {
                let effect = effect::make(voice).unwrap_or_else(|| Box::new(effect::AsItIs));
                let _ = made.send((voice, effect));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the voice cannot be made");
            self.making = None;
        }
    }

    /// Read or not: a link goes out of the source's node.
    fn reading(&mut self) {
        let read = self
            .node
            .is_some_and(|node| self.links.values().any(|(from, _)| *from == node));
        if read == self.read {
            return;
        }
        self.read = read;
        tracing::info!(read, "the source");
        self.tell(FromVoice::Read(read));
        if read {
            lock(&self.engine).clear();
            self.start_reading();
        } else {
            self.stop_reading();
        }
    }

    /// The microphone to read ([`choose`]).
    fn choose(&self) -> Option<u32> {
        choose(
            &self.nodes,
            self.default.as_deref(),
            self.configured.as_deref(),
            |id, node| {
                let place = self.places.get(&id);
                !node
                    .device
                    .zip(place.copied())
                    .is_some_and(|route| self.empty.contains(&route))
            },
        )
    }

    /// The microphone it would read now said when it changed; read again
    /// when it changed while read.
    fn microphone_changed(&mut self) {
        let chosen = self.choose();
        let mic = chosen
            .and_then(|id| self.nodes.get(&id))
            .map(|n| Microphone {
                name: n.name.clone(),
                description: n.description.clone(),
            });
        if self.told.as_ref() != Some(&mic) {
            tracing::info!(?mic, "the microphone");
            self.tell(FromVoice::Microphone(mic.clone()));
            self.told = Some(mic);
        }
        if !self.read {
            return;
        }
        if self
            .input
            .as_ref()
            .is_some_and(|i| Some(i.microphone) != chosen)
        {
            self.stop_input();
        }
        if self.input.is_none() {
            self.start_reading();
        }
    }

    fn start_reading(&mut self) {
        if self.input.is_some() {
            return;
        }
        let Some(microphone) = self.choose() else {
            tracing::info!("no microphone to read");
            return;
        };
        match self.read_microphone(microphone) {
            Ok(input) => {
                tracing::info!(microphone, "the microphone is read");
                self.input = Some(input);
            }
            Err(err) => tracing::warn!(%err, microphone, "the microphone cannot be read"),
        }
    }

    fn read_microphone(&self, microphone: u32) -> Result<In, pw::Error> {
        let stream = pw::stream::StreamRc::new(
            self.core.clone(),
            VOICE_INPUT_NODE,
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Communication",
                *pw::keys::NODE_NAME => VOICE_INPUT_NODE,
                *pw::keys::APP_NAME => "Hocuspanda",
                *pw::keys::NODE_LATENCY => LATENCY,
                "node.autoconnect" => "false",
                "node.dont-reconnect" => "true",
                "node.group" => GROUP,
                "node.link-group" => GROUP,
                "adapter.auto-port-config" => "{ mode = dsp monitor = false position = preserve }",
            },
        )?;
        let states = self.dispatcher();
        let engine = self.engine.clone();
        let mut samples = Vec::with_capacity(2 * CHUNK);
        let listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, old, new| {
                tracing::debug!(?old, ?new, "the microphone's stream");
                states(Ev::InState(new));
            })
            .process(move |stream, _| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                samples_in(&mut buffer, &mut samples);
                lock(&engine).heard(&samples);
            })
            .register()?;
        let pod = format_pod();
        let pod = Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
        stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
            &mut [pod],
        )?;
        Ok(In {
            stream,
            _listener: listener,
            microphone,
            node: None,
            link: None,
            asked: 0,
        })
    }

    /// The microphone's first output port linked to the stream's input
    /// port, once both are on the graph; linked again when either is made
    /// again (a session manager setting the node up) or the link goes.
    fn link(&mut self) {
        let Some(input) = &mut self.input else {
            return;
        };
        let Some(node) = input.node.filter(|n| *n != 0 && *n != u32::MAX) else {
            return;
        };
        // Its own ports first; a monitor's only when it has none (a null
        // source's sound comes out of its monitor). The first channel's.
        let find = |of: u32, output: bool| {
            let ports = || {
                self.ports
                    .iter()
                    .filter(|(_, p)| p.node == of && p.output == output)
            };
            let first = |(id, p): (&u32, &Port)| (p.index, *id);
            ports()
                .filter(|(_, p)| !p.monitor)
                .map(first)
                .min()
                .or_else(|| ports().map(first).min())
                .map(|(_, id)| id)
        };
        let (Some(from), Some(to)) = (find(input.microphone, true), find(node, false)) else {
            // Its ports being made again: the link went with them.
            input.link = None;
            return;
        };
        if input.link.as_ref().is_some_and(|l| l.ports == (from, to)) {
            return;
        }
        // To ports gone, or none yet.
        input.link = None;
        if input.asked >= MOST_LINKS {
            if input.asked == MOST_LINKS {
                tracing::warn!(
                    microphone = input.microphone,
                    asked = input.asked,
                    "the microphone's link keeps going: no longer asked"
                );
                input.asked += 1;
            }
            return;
        }
        input.asked += 1;
        match self.core.create_object::<pw::link::Link>(
            "link-factory",
            &properties! {
                *pw::keys::LINK_OUTPUT_NODE => input.microphone.to_string(),
                *pw::keys::LINK_OUTPUT_PORT => from.to_string(),
                *pw::keys::LINK_INPUT_NODE => node.to_string(),
                *pw::keys::LINK_INPUT_PORT => to.to_string(),
            },
        ) {
            Ok(proxy) => {
                tracing::debug!(
                    microphone = input.microphone,
                    node,
                    ports = ?(from, to),
                    "a link to the microphone asked"
                );
                input.link = Some(MicLink {
                    proxy,
                    ports: (from, to),
                    state: LinkState::Asked,
                });
            }
            Err(err) => tracing::warn!(%err, "the microphone could not be linked"),
        }
    }

    fn stop_input(&mut self) {
        if let Some(input) = self.input.take() {
            drop(input.link);
            let _ = input.stream.disconnect();
            tracing::info!(
                microphone = input.microphone,
                "the microphone is no longer read"
            );
        }
    }

    /// Nothing reads: the microphone let go, the voice back to its start.
    fn stop_reading(&mut self) {
        self.stop_input();
        lock(&self.engine).clear();
    }
}

/// The microphone to read: the default source unless it is this one, else
/// the one chosen, else the one of highest priority; among those `heard`
/// says something can be heard on (a jack's microphone is not while its
/// jack is empty), unless none can.
fn choose(
    nodes: &BTreeMap<u32, Node>,
    default: Option<&str>,
    configured: Option<&str>,
    heard: impl Fn(u32, &Node) -> bool,
) -> Option<u32> {
    let mics = || nodes.iter().filter(|(_, n)| n.microphone());
    let any_heard = mics().any(|(id, n)| heard(*id, n));
    let mics = || mics().filter(|(id, n)| !any_heard || heard(**id, n));
    let named = |name: Option<&str>| {
        let name = name?;
        mics().find(|(_, n)| n.name == name).map(|(id, _)| *id)
    };
    named(default).or_else(|| named(configured)).or_else(|| {
        mics()
            .max_by_key(|(id, n)| (n.priority, std::cmp::Reverse(**id)))
            .map(|(id, _)| *id)
    })
}

/// A Route param's place (`device`) and whether something can be heard on
/// it: all but `available` no (unknown is a built-in microphone's).
fn route_of(param: &Pod) -> Option<(i32, bool)> {
    let route = param.as_object().ok()?;
    let place = route
        .find_prop(spa::utils::Id(spa::sys::SPA_PARAM_ROUTE_device))?
        .value()
        .get_int()
        .ok()?;
    let available = route
        .find_prop(spa::utils::Id(spa::sys::SPA_PARAM_ROUTE_available))
        .and_then(|p| p.value().get_id().ok());
    Some((
        place,
        available != Some(spa::utils::Id(spa::sys::SPA_PARAM_AVAILABILITY_no)),
    ))
}

/// A metadata's JSON value's `name` (`{"name":"alsa_input…"}`).
fn name_of(value: &str) -> Option<String> {
    let at = value.find("\"name\"")?;
    let rest = value
        .get(at + 6..)?
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest.get(..end)?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_s_name_is_read() {
        assert_eq!(
            name_of(r#"{ "name": "alsa_input.pci-0000_00_1f.3.analog-stereo" }"#).as_deref(),
            Some("alsa_input.pci-0000_00_1f.3.analog-stereo")
        );
        assert_eq!(name_of("{}"), None);
    }

    fn mic(name: &str, priority: i64, device: Option<u32>) -> Node {
        Node {
            name: name.to_owned(),
            class: "Audio/Source".to_owned(),
            description: name.to_owned(),
            priority,
            device,
        }
    }

    /// The laptop of #911: the Hocuspanda microphone made the default, the
    /// headset's jack (priority 2000) empty, the built-in one (1648) heard.
    fn laptop() -> BTreeMap<u32, Node> {
        BTreeMap::from([
            (55, mic("alsa_input.HiFi__Mic1__source", 1648, Some(49))),
            (80, mic("alsa_input.HiFi__Mic2__source", 2000, Some(49))),
            (90, mic(VOICE_NODE, 100, None)),
        ])
    }

    #[test]
    fn a_microphone_whose_jack_is_empty_is_never_read_while_another_is_heard() {
        let nodes = laptop();
        let voice = Some(VOICE_NODE);
        // The headset's jack empty: the built-in microphone, though lower.
        let jack_empty = |id: u32, _: &Node| id != 80;
        assert_eq!(choose(&nodes, voice, voice, jack_empty), Some(55));
        // Not even when it is the one chosen.
        let chosen = Some("alsa_input.HiFi__Mic2__source");
        assert_eq!(choose(&nodes, voice, chosen, jack_empty), Some(55));
        // A headset plugged in: the highest again.
        assert_eq!(choose(&nodes, voice, voice, |_, _| true), Some(80));
        // Nothing heard anywhere: the highest all the same, never none.
        assert_eq!(choose(&nodes, voice, voice, |_, _| false), Some(80));
        // The session's default, when it is a microphone, as before.
        let default = Some("alsa_input.HiFi__Mic1__source");
        assert_eq!(choose(&nodes, default, voice, |_, _| true), Some(55));
    }

    #[test]
    fn a_route_says_its_place_and_whether_it_is_heard() {
        use spa::pod::{Object, Property, Value, serialize::PodSerializer};
        let route = |available: u32| {
            let object = Object {
                type_: spa::sys::SPA_TYPE_OBJECT_ParamRoute,
                id: spa::sys::SPA_PARAM_Route,
                properties: vec![
                    Property::new(spa::sys::SPA_PARAM_ROUTE_index, Value::Int(3)),
                    Property::new(spa::sys::SPA_PARAM_ROUTE_device, Value::Int(2)),
                    Property::new(
                        spa::sys::SPA_PARAM_ROUTE_available,
                        Value::Id(spa::utils::Id(available)),
                    ),
                ],
            };
            PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(object))
                .map(|(cursor, _)| cursor.into_inner())
                .unwrap_or_default()
        };
        let read = |bytes: Vec<u8>| Pod::from_bytes(&bytes).and_then(route_of);
        assert_eq!(
            read(route(spa::sys::SPA_PARAM_AVAILABILITY_no)),
            Some((2, false))
        );
        assert_eq!(
            read(route(spa::sys::SPA_PARAM_AVAILABILITY_unknown)),
            Some((2, true))
        );
        assert_eq!(
            read(route(spa::sys::SPA_PARAM_AVAILABILITY_yes)),
            Some((2, true))
        );
    }

    /// The source's quantum before the microphone's in a cycle takes the
    /// last cycle's: the queue keeps one quantum, no more.
    #[test]
    fn the_queue_keeps_one_quantum_whatever_the_order() {
        let mut engine = Engine::default();
        let mut out = [0f32; 256];
        let quantum: Vec<f32> = (0..256).map(|i| i as f32 / 256.0).collect();
        // The source first, then the microphone, each cycle.
        for _ in 0..10 {
            engine.said(&mut out);
            engine.heard(&quantum);
        }
        assert_eq!(engine.queue.len(), 256);
        assert_eq!(out.to_vec(), quantum);
        // The microphone first: nothing waits.
        let mut engine = Engine::default();
        for _ in 0..10 {
            engine.heard(&quantum);
            engine.said(&mut out);
        }
        assert!(engine.queue.is_empty());
    }
}
