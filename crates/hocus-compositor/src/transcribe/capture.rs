//! A meeting's sound, off the loop: two tracks from PipeWire, never mixed,
//! 16 kHz mono, written as WAV files in the meeting's folder at the
//! recording's own pace (as many samples as it has lasted, silence where
//! nothing came).
//!
//! - **Moi**: the default source, as the recording's microphone is (the
//!   session manager links it).
//! - **The call**: the output streams of the application in the call (by
//!   its streams' key, `hocus_sound::app_key`), linked to this stream here,
//!   as they come and go: not the whole desktop's sound, so music and
//!   notifications stay out. The stream makes its own ports
//!   (`adapter.auto-port-config`), so that it can be linked without a
//!   session manager too.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_meetings::{RATE, Record, Track, wav};
use pipewire as pw;
use pw::{properties::properties, spa, types::ObjectType};
use smithay::reexports::calloop::channel::Sender;

use super::FromWorkers;

/// The tracks' streams' node names (`hocus_sound::TRANSCRIPTION_NODES`).
pub const ME_NODE: &str = "hocuspanda-transcribe-me";
pub const CALL_NODE: &str = "hocuspanda-transcribe-call";
/// How often the writer writes.
const EVERY: Duration = Duration::from_millis(100);
/// The most a stream's queue keeps: two seconds.
const MOST: usize = 2 * RATE as usize;

type Queue = Arc<Mutex<VecDeque<f32>>>;

/// A meeting's recording, as the loop holds it: stopping it never waits.
pub struct Capture {
    quit: Option<pw::channel::Sender<()>>,
    done: Arc<AtomicBool>,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Capture")
    }
}

/// What a recording is made of.
#[derive(Debug, Clone)]
pub struct Plan {
    pub id: u64,
    pub dir: PathBuf,
    pub record: Record,
    /// The call's application's key.
    pub app: String,
    pub remote: Option<PathBuf>,
}

impl Capture {
    /// The recording of `plan` starts: its folder, record and files made,
    /// its streams on PipeWire, on threads of their own; `back` told when
    /// its files are ended ([`FromWorkers::Recorded`]).
    pub fn start(plan: Plan, back: Sender<FromWorkers>) -> anyhow::Result<Self> {
        let (me, call) = (Queue::default(), Queue::default());
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let (remote, app) = (plan.remote.clone(), plan.app.clone());
        let (me_in, call_in) = (me.clone(), call.clone());
        std::thread::Builder::new()
            .name("meeting's sound".to_owned())
            .spawn(move || {
                if let Err(err) = streams(remote, &app, &me_in, &call_in, quit_rx) {
                    tracing::warn!(%err, "no sound from PipeWire: the meeting's tracks are silent");
                }
            })?;
        let done = Arc::new(AtomicBool::new(false));
        let writer_done = done.clone();
        std::thread::Builder::new()
            .name("meeting's tracks".to_owned())
            .spawn(move || {
                let id = plan.id;
                let written = write(&plan, &me, &call, &writer_done);
                if let Err(err) = &written {
                    tracing::warn!(id, %err, dir = %plan.dir.display(), "the meeting's tracks cannot be written");
                }
                let _ = back.send(FromWorkers::Recorded {
                    id,
                    ok: written.is_ok(),
                });
            })?;
        Ok(Self {
            quit: Some(quit),
            done,
        })
    }

    /// It stops: the writer ends its files, then says so; the streams go.
    pub fn stop(mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(quit) = self.quit.take() {
            let _ = quit.send(());
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(quit) = self.quit.take() {
            let _ = quit.send(());
        }
    }
}

/// The writer: the folder, its record and its tracks, then every
/// [`EVERY`] what the recording owes each track, until `done`.
fn write(plan: &Plan, me: &Queue, call: &Queue, done: &AtomicBool) -> std::io::Result<()> {
    std::fs::create_dir_all(&plan.dir)?;
    plan.record.write(&plan.dir)?;
    let mut tracks = [
        (me, wav::Writer::create(&plan.dir.join(Track::Me.file()))?),
        (
            call,
            wav::Writer::create(&plan.dir.join(Track::Call.file()))?,
        ),
    ];
    let started = Instant::now();
    let mut written = 0u64;
    loop {
        let finishing = done.load(Ordering::Acquire);
        let owed = (started.elapsed().as_secs_f64() * f64::from(RATE)) as u64;
        let n = usize::try_from(owed.saturating_sub(written)).unwrap_or(0);
        for (queue, writer) in &mut tracks {
            let mut samples = {
                let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
                let have = queue.len().min(n);
                queue.drain(..have).collect::<Vec<f32>>()
            };
            samples.resize(n, 0.0);
            writer.write(&samples)?;
        }
        written += n as u64;
        if finishing {
            break;
        }
        std::thread::sleep(EVERY);
    }
    for (_, writer) in tracks {
        writer.finish()?;
    }
    tracing::info!(
        id = plan.id,
        seconds = written / u64::from(RATE),
        "the meeting's tracks are written"
    );
    Ok(())
}

/// The graph as this thread follows it: the call's application's output
/// nodes and their ports, the call stream's own input ports, the links
/// made.
#[derive(Default)]
struct Graph {
    factory: Option<String>,
    app_nodes: HashSet<u32>,
    call_node: Option<u32>,
    /// Output ports by id: their node, whether a monitor's.
    outs: HashMap<u32, (u32, bool)>,
    /// The call stream's input ports.
    ins: Vec<u32>,
    linked: HashSet<(u32, u32)>,
    links: Vec<pw::link::Link>,
}

impl Graph {
    /// Every port of the call's application linked to the call stream's
    /// ports: its own outputs, else its monitors'.
    fn links_wanted(&self) -> Vec<(u32, u32, u32, u32)> {
        let Some(call) = self.call_node else {
            return Vec::new();
        };
        let mut wanted = Vec::new();
        for node in &self.app_nodes {
            let ports: Vec<(u32, bool)> = self
                .outs
                .iter()
                .filter(|(_, (n, _))| n == node)
                .map(|(port, (_, monitor))| (*port, *monitor))
                .collect();
            let plain = ports.iter().any(|(_, monitor)| !monitor);
            for (port, monitor) in ports {
                if monitor && plain {
                    continue;
                }
                for into in &self.ins {
                    if !self.linked.contains(&(port, *into)) {
                        wanted.push((*node, port, call, *into));
                    }
                }
            }
        }
        wanted
    }
}

/// The two streams on PipeWire's `remote` (its default otherwise), their
/// samples into the queues, the call's linked as its application's
/// streams come, until `quit`.
fn streams(
    remote: Option<PathBuf>,
    app: &str,
    me: &Queue,
    call: &Queue,
    quit: pw::channel::Receiver<()>,
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
    let me_stream = stream(&core, ME_NODE, me.clone())?;
    let call_stream = stream(&core, CALL_NODE, call.clone())?;
    let registry = core.get_registry_rc()?;
    let graph = Rc::new(RefCell::new(Graph::default()));
    let app = app.to_owned();
    let (seen, gone) = (graph.clone(), graph.clone());
    let link_core = core.clone();
    let _listener = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props else {
                return;
            };
            let mut graph = seen.borrow_mut();
            match global.type_ {
                ObjectType::Factory => {
                    if props.get("factory.type.name") == Some(ObjectType::Link.to_str()) {
                        graph.factory = props.get("factory.name").map(str::to_owned);
                    }
                }
                ObjectType::Node => {
                    let name = props.get("node.name").unwrap_or_default();
                    if name == CALL_NODE {
                        graph.call_node = Some(global.id);
                    } else if props.get("media.class") == Some("Stream/Output/Audio")
                        && hocus_sound::app_key(
                            props.get("pipewire.access.portal.app_id"),
                            props.get("application.name"),
                            props.get("application.process.binary"),
                            name,
                        ) == app
                    {
                        tracing::info!(node = global.id, "the call's stream is heard");
                        graph.app_nodes.insert(global.id);
                    }
                }
                ObjectType::Port => {
                    let node = props.get("node.id").and_then(|n| n.parse::<u32>().ok());
                    let Some(node) = node else {
                        return;
                    };
                    match props.get("port.direction") {
                        Some("out") => {
                            let monitor = props.get("port.monitor") == Some("true");
                            graph.outs.insert(global.id, (node, monitor));
                        }
                        Some("in") if Some(node) == graph.call_node => graph.ins.push(global.id),
                        _ => {}
                    }
                }
                _ => return,
            }
            link(&link_core, &mut graph);
        })
        .global_remove(move |id| {
            let mut graph = gone.borrow_mut();
            graph.app_nodes.remove(&id);
            graph.outs.remove(&id);
            graph.ins.retain(|p| *p != id);
            graph.linked.retain(|(out, into)| *out != id && *into != id);
        })
        .register();
    let weak = mainloop.downgrade();
    let _quit = quit.attach(mainloop.loop_(), move |()| {
        if let Some(mainloop) = weak.upgrade() {
            mainloop.quit();
        }
    });
    mainloop.run();
    let _ = me_stream.0.disconnect();
    let _ = call_stream.0.disconnect();
    drop(graph);
    Ok(())
}

/// The links the graph lacks, made: dropped with this thread (they do not
/// linger).
fn link(core: &pw::core::CoreRc, graph: &mut Graph) {
    let factory = graph
        .factory
        .clone()
        .unwrap_or_else(|| "link-factory".to_owned());
    for (out_node, out_port, in_node, in_port) in graph.links_wanted() {
        let made = core.create_object::<pw::link::Link>(
            &factory,
            &properties! {
                "link.output.node" => out_node.to_string(),
                "link.output.port" => out_port.to_string(),
                "link.input.node" => in_node.to_string(),
                "link.input.port" => in_port.to_string(),
                "object.linger" => "false",
            },
        );
        match made {
            Ok(link) => {
                tracing::info!(out_port, in_port, "the call's port is linked to its track");
                graph.linked.insert((out_port, in_port));
                graph.links.push(link);
            }
            Err(err) => tracing::warn!(%err, out_port, "the call's port cannot be linked"),
        }
    }
}

/// A capture stream `node`, its samples into `queue`: the microphone's
/// connected by the session manager to the default source; the call's
/// left to [`link`], its ports made by itself.
fn stream(
    core: &pw::core::CoreRc,
    node: &str,
    queue: Queue,
) -> Result<(pw::stream::StreamRc, pw::stream::StreamListener<()>), pw::Error> {
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => node.to_owned(),
        *pw::keys::APP_NAME => "Hocuspanda",
        *pw::keys::NODE_DESCRIPTION => if node == ME_NODE {
            "Hocuspanda: transcribing the microphone"
        } else {
            "Hocuspanda: transcribing the call"
        },
    };
    let call = node == CALL_NODE;
    if call {
        props.insert("node.autoconnect", "false");
        props.insert(
            "adapter.auto-port-config",
            "{ mode = dsp monitor = false position = preserve }",
        );
    }
    let stream = pw::stream::StreamRc::new(core.clone(), node, props)?;
    let listener = stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let size = data.chunk().size() as usize;
            let offset = data.chunk().offset() as usize;
            if let Some(bytes) = data.data() {
                let end = (offset + size).min(bytes.len());
                let bytes = bytes.get(offset..end).unwrap_or_default();
                let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
                queue.extend(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
                let extra = queue.len().saturating_sub(MOST);
                queue.drain(..extra);
            }
        })
        .register()?;
    let pod = format_pod();
    let pod = spa::pod::Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
    let mut params = [pod];
    let mut flags = pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS;
    if !call {
        flags |= pw::stream::StreamFlags::AUTOCONNECT;
    }
    stream.connect(spa::utils::Direction::Input, None, flags, &mut params)?;
    tracing::info!(node, "a meeting's track's stream");
    Ok((stream, listener))
}

/// Raw audio, 32-bit floats, 16 kHz, mono: PipeWire converts to it.
fn format_pod() -> Vec<u8> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_own_ports_are_linked_its_monitors_only_without_them() {
        let mut graph = Graph {
            call_node: Some(50),
            ins: vec![51],
            ..Graph::default()
        };
        graph.app_nodes.insert(10);
        graph.app_nodes.insert(20);
        // Node 10: its own outputs and its monitors; node 20: monitors
        // only (a null node, as tests make).
        graph.outs.insert(11, (10, false));
        graph.outs.insert(12, (10, true));
        graph.outs.insert(21, (20, true));
        graph.outs.insert(31, (30, false));
        let mut wanted = graph.links_wanted();
        wanted.sort_unstable();
        assert_eq!(wanted, vec![(10, 11, 50, 51), (20, 21, 50, 51)]);
        graph.linked.insert((11, 51));
        assert_eq!(graph.links_wanted(), vec![(20, 21, 50, 51)]);
        graph.call_node = None;
        assert!(graph.links_wanted().is_empty());
    }
}
