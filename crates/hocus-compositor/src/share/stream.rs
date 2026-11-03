//! The shares' video streams on PipeWire, on a thread of its own with
//! PipeWire's loop: one output stream per share, a driver (the desktop
//! paces it, a frame when something changed), its buffers in shared
//! memory that PipeWire allocates (`MemFd`), its pixels B, G, R, x (or A).
//!
//! The loop draws a share's frame and reads it back; the pixels come
//! here, owned, and are copied into a buffer the consumer has given back,
//! if one is free (else the frame is dropped: the consumer is behind).
//! The pixels' memory goes back to the loop to be drawn into again. What
//! happens to a stream (its node, its format, a consumer streaming or
//! not) goes back to the loop. Nothing here waits for the loop, nor the
//! loop for PipeWire.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};

use pipewire as pw;
use pw::{properties::properties, spa};
use smithay::reexports::calloop::channel::Sender;
use spa::pod::{ChoiceValue, Object, Pod, Property, PropertyFlags, Value};
use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Rectangle};

use super::lease::{Lease, LoopCopies};

/// The most frames a second a consumer may ask for.
pub const MOST_FPS: u32 = 60;

/// What the loop tells the streams' thread.
#[derive(Debug)]
pub enum ToStreams {
    /// A stream for share `id`, `size` pixels, named for what it shows.
    Create {
        id: u64,
        size: (u32, u32),
        name: String,
    },
    /// Its source changed size: its format is offered again.
    Resize {
        id: u64,
        size: (u32, u32),
    },
    /// A frame: its pixels lent (rows of `stride` bytes, `size` pixels),
    /// given back once copied.
    Frame {
        id: u64,
        lease: Lease,
        size: (u32, u32),
        stride: u32,
    },
    Destroy {
        id: u64,
    },
    /// The compositor ends: the thread too.
    Quit,
}

/// What the streams' thread tells the loop.
#[derive(Debug)]
pub enum FromStreams {
    /// The stream is on the graph, as this node.
    Ready {
        id: u64,
        node: u32,
    },
    Failed {
        id: u64,
        why: String,
    },
    /// A consumer agreed on this size and pace.
    Format {
        id: u64,
        size: (u32, u32),
        fps: u32,
    },
    /// A consumer streams from it (`on`), or stopped.
    Streaming {
        id: u64,
        on: bool,
    },
    /// A frame's pixels lent are given back: its memory may be written
    /// again.
    Returned {
        id: u64,
    },
    /// A frame found no free buffer (the consumer was not ready, or is
    /// behind): one is owed.
    Dropped {
        id: u64,
    },
}

/// The way to the thread, and back to the loop: the read-back thread
/// holds one too.
#[derive(Clone)]
pub struct StreamsSender {
    to: pw::channel::Sender<ToStreams>,
    back: Sender<FromStreams>,
    /// The bytes of frames read on the loop: none, ever.
    copies: Arc<LoopCopies>,
}

impl std::fmt::Debug for StreamsSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamsSender")
    }
}

impl StreamsSender {
    pub fn send(&self, said: ToStreams) {
        if self.to.send(said).is_err() {
            tracing::warn!("the streams' thread is gone");
        }
    }

    /// Tells the loop, as the streams' thread would.
    pub fn tell(&self, said: FromStreams) {
        let _ = self.back.send(said);
    }

    /// `pixels`, lent to the streams (see [`Lease::new`]).
    pub fn lend(&self, pixels: &[u8]) -> Lease {
        Lease::new(pixels, self.copies.clone())
    }

    /// The same, the lender telling the loop once they are free.
    pub fn lend_quiet(&self, pixels: &[u8]) -> Lease {
        Lease::quiet(pixels, self.copies.clone())
    }

    /// The bytes of shared frames read (so copied) on the loop's thread.
    pub fn copied_on_loop(&self) -> u64 {
        self.copies.bytes()
    }
}

/// The thread, as the loop holds it: it ends with it.
#[derive(Debug)]
pub struct Streams {
    sender: StreamsSender,
}

impl Streams {
    pub fn send(&self, said: ToStreams) {
        self.sender.send(said);
    }

    pub fn sender(&self) -> &StreamsSender {
        &self.sender
    }
}

impl Drop for Streams {
    fn drop(&mut self) {
        let _ = self.sender.to.send(ToStreams::Quit);
    }
}

/// Starts the thread, on PipeWire's socket `remote` (its default
/// otherwise: `PIPEWIRE_REMOTE`, else `pipewire-0` in the runtime
/// directory). Called on the loop, whose thread the leases' reads are
/// counted on.
pub fn start(remote: Option<PathBuf>, back: Sender<FromStreams>) -> Option<Streams> {
    let (to, from_loop) = pw::channel::channel::<ToStreams>();
    let tell = back.clone();
    let started = std::thread::Builder::new()
        .name("screen cast streams".to_owned())
        .spawn(move || {
            if let Err(err) = run(remote, from_loop, back.clone()) {
                tracing::warn!(%err, "PipeWire is not there: nothing is streamed");
                let _ = back.send(FromStreams::Failed {
                    id: 0,
                    why: err.to_string(),
                });
            }
        });
    match started {
        Ok(_) => Some(Streams {
            sender: StreamsSender {
                to,
                back: tell,
                copies: LoopCopies::here(),
            },
        }),
        Err(err) => {
            tracing::warn!(%err, "cannot start the streams' thread");
            None
        }
    }
}

struct Live {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
    /// The size agreed (else offered), and its rows' bytes.
    size: Rc<RefCell<(u32, u32)>>,
}

fn run(
    remote: Option<PathBuf>,
    from_loop: pw::channel::Receiver<ToStreams>,
    back: Sender<FromStreams>,
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
    let lives: Rc<RefCell<HashMap<u64, Live>>> = Rc::default();
    let weak = mainloop.downgrade();
    let _receiver = from_loop.attach(mainloop.loop_(), move |said| match said {
        ToStreams::Create { id, size, name } => {
            match create(&core, id, size, &name, back.clone()) {
                Ok(live) => {
                    lives.borrow_mut().insert(id, live);
                }
                Err(err) => {
                    let _ = back.send(FromStreams::Failed {
                        id,
                        why: err.to_string(),
                    });
                }
            }
        }
        ToStreams::Resize { id, size } => {
            if let Some(live) = lives.borrow().get(&id) {
                *live.size.borrow_mut() = size;
                let pod = format_pod(size);
                if let Some(pod) = Pod::from_bytes(&pod) {
                    let mut params = [pod];
                    if let Err(err) = live.stream.update_params(&mut params) {
                        tracing::warn!(%err, id, "the stream's new size not offered");
                    }
                }
            }
        }
        ToStreams::Frame {
            id,
            lease,
            size,
            stride,
        } => {
            let queued = lives.borrow().get(&id).is_some_and(|live| {
                lease
                    .with(|pixels| queue(live, pixels, size, stride))
                    .unwrap_or(false)
            });
            lease.give_back();
            // The read-back thread says it itself, once it unmapped them.
            if !lease.is_quiet() {
                let _ = back.send(FromStreams::Returned { id });
            }
            if !queued {
                let _ = back.send(FromStreams::Dropped { id });
            }
        }
        ToStreams::Destroy { id } => {
            if let Some(live) = lives.borrow_mut().remove(&id) {
                let _ = live.stream.disconnect();
                tracing::info!(id, "a share's stream ends");
            }
        }
        ToStreams::Quit => {
            for (_, live) in lives.borrow_mut().drain() {
                let _ = live.stream.disconnect();
            }
            if let Some(mainloop) = weak.upgrade() {
                mainloop.quit();
            }
        }
    });
    mainloop.run();
    Ok(())
}

/// The format offered: raw video, B, G, R and x (or A), of `size`, at any
/// pace up to [`MOST_FPS`] (the desktop sends a frame when something
/// changed).
fn format_pod((width, height): (u32, u32)) -> Vec<u8> {
    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    use spa::param::video::VideoFormat;
    let prop = |key: FormatProperties, value: Value| Property {
        key: key.as_raw(),
        flags: PropertyFlags::empty(),
        value,
    };
    let id = |v: u32| Value::Id(spa::utils::Id(v));
    let object = Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: vec![
            prop(FormatProperties::MediaType, id(MediaType::Video.as_raw())),
            prop(
                FormatProperties::MediaSubtype,
                id(MediaSubtype::Raw.as_raw()),
            ),
            prop(
                FormatProperties::VideoFormat,
                Value::Choice(ChoiceValue::Id(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Enum {
                        default: spa::utils::Id(VideoFormat::BGRx.as_raw()),
                        alternatives: vec![
                            spa::utils::Id(VideoFormat::BGRx.as_raw()),
                            spa::utils::Id(VideoFormat::BGRA.as_raw()),
                        ],
                    },
                ))),
            ),
            prop(
                FormatProperties::VideoSize,
                Value::Rectangle(Rectangle { width, height }),
            ),
            prop(
                FormatProperties::VideoFramerate,
                Value::Fraction(Fraction { num: 0, denom: 1 }),
            ),
            prop(
                FormatProperties::VideoMaxFramerate,
                Value::Choice(ChoiceValue::Fraction(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: Fraction {
                            num: MOST_FPS,
                            denom: 1,
                        },
                        min: Fraction { num: 1, denom: 1 },
                        max: Fraction {
                            num: MOST_FPS,
                            denom: 1,
                        },
                    },
                ))),
            ),
        ],
    };
    serialize(object)
}

/// The buffers asked for once the format is agreed: 2 to 8 of one block,
/// `stride` × `height` bytes, in shared memory.
fn buffers_pod(stride: u32, height: u32) -> Vec<u8> {
    let prop = |key: u32, value: Value| Property {
        key,
        flags: PropertyFlags::empty(),
        value,
    };
    let int = |v: u32| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let memory = (1 << spa::sys::SPA_DATA_MemFd) | (1 << spa::sys::SPA_DATA_MemPtr);
    let object = Object {
        type_: spa::utils::SpaTypes::ObjectParamBuffers.as_raw(),
        id: spa::param::ParamType::Buffers.as_raw(),
        properties: vec![
            prop(
                spa::sys::SPA_PARAM_BUFFERS_buffers,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: 4,
                        min: 2,
                        max: 8,
                    },
                ))),
            ),
            prop(spa::sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
            prop(
                spa::sys::SPA_PARAM_BUFFERS_size,
                int(stride.saturating_mul(height)),
            ),
            prop(spa::sys::SPA_PARAM_BUFFERS_stride, int(stride)),
            prop(
                spa::sys::SPA_PARAM_BUFFERS_dataType,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags {
                        default: memory,
                        flags: vec![memory],
                    },
                ))),
            ),
        ],
    };
    serialize(object)
}

fn serialize(object: Object) -> Vec<u8> {
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .unwrap_or_default()
}

fn create(
    core: &pw::core::CoreRc,
    id: u64,
    size: (u32, u32),
    name: &str,
    back: Sender<FromStreams>,
) -> Result<Live, pw::Error> {
    let stream = pw::stream::StreamRc::new(
        core.clone(),
        "hocuspanda-screen-cast",
        properties! {
            *pw::keys::MEDIA_CLASS => "Video/Source",
            *pw::keys::MEDIA_ROLE => "Screen",
            *pw::keys::NODE_NAME => "hocuspanda-screen-cast",
            *pw::keys::NODE_DESCRIPTION => name.to_owned(),
        },
    )?;
    let agreed = Rc::new(RefCell::new(size));
    let ready = back.clone();
    let formats = back.clone();
    let format_size = agreed.clone();
    let listener = stream
        .add_local_listener::<()>()
        .state_changed(move |stream, _, old, new| {
            tracing::debug!(id, ?old, ?new, "a share's stream");
            use pw::stream::StreamState;
            match (&old, &new) {
                (StreamState::Connecting, StreamState::Paused) => {
                    let _ = ready.send(FromStreams::Ready {
                        id,
                        node: stream.node_id(),
                    });
                }
                (_, StreamState::Streaming) => {
                    let _ = ready.send(FromStreams::Streaming { id, on: true });
                }
                (StreamState::Streaming, _) => {
                    let _ = ready.send(FromStreams::Streaming { id, on: false });
                }
                (_, StreamState::Error(why)) => {
                    let _ = ready.send(FromStreams::Failed {
                        id,
                        why: why.clone(),
                    });
                }
                _ => {}
            }
        })
        .param_changed(move |stream, _, param_id, param| {
            let Some(param) = param else {
                return;
            };
            if param_id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::default();
            if info.parse(param).is_err() {
                return;
            }
            let size = (info.size().width, info.size().height);
            let max = info.max_framerate();
            let fps = if max.denom == 0 || max.num == 0 {
                MOST_FPS
            } else {
                (max.num / max.denom).clamp(1, MOST_FPS)
            };
            tracing::info!(
                id,
                format = ?info.format(),
                width = size.0,
                height = size.1,
                fps,
                "a share's stream agreed"
            );
            *format_size.borrow_mut() = size;
            let pod = buffers_pod(size.0.saturating_mul(4), size.1);
            if let Some(pod) = Pod::from_bytes(&pod) {
                let mut params = [pod];
                if let Err(err) = stream.update_params(&mut params) {
                    tracing::warn!(%err, id, "the stream's buffers not asked");
                }
            }
            let _ = formats.send(FromStreams::Format { id, size, fps });
        })
        .register()?;
    let pod = format_pod(size);
    let pod = Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
    let mut params = [pod];
    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::DRIVER | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;
    tracing::info!(
        id,
        name,
        width = size.0,
        height = size.1,
        "a share's stream"
    );
    Ok(Live {
        stream,
        _listener: listener,
        size: agreed,
    })
}

/// A frame into a free buffer of `live`'s, if it streams and one is free:
/// whether it went.
fn queue(live: &Live, pixels: &[u8], size: (u32, u32), stride: u32) -> bool {
    if live.stream.state() != pw::stream::StreamState::Streaming {
        return false;
    }
    let agreed = *live.size.borrow();
    let Some(mut buffer) = live.stream.dequeue_buffer() else {
        tracing::trace!("no free buffer: a frame dropped");
        return false;
    };
    let datas = buffer.datas_mut();
    let Some(data) = datas.first_mut() else {
        return false;
    };
    let out_stride = agreed.0.saturating_mul(4) as usize;
    let rows = agreed.1.min(size.1) as usize;
    let row = (agreed.0.min(size.0) as usize) * 4;
    let mut written = 0;
    if let Some(memory) = data.data() {
        let stride = stride as usize;
        for y in 0..rows {
            let (from, to) = (y * stride, y * out_stride);
            if from + row > pixels.len() || to + row > memory.len() {
                break;
            }
            memory[to..to + row].copy_from_slice(&pixels[from..from + row]);
            written = to + out_stride;
        }
    }
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = i32::try_from(out_stride).unwrap_or(i32::MAX);
    *chunk.size_mut() = u32::try_from(written).unwrap_or(u32::MAX);
    drop(buffer);
    if live.stream.is_driving() {
        let _ = live.stream.trigger_process();
    }
    written > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_offered_reads_back() {
        let pod = format_pod((2560, 1600));
        assert!(Pod::from_bytes(&pod).is_some());
        let buffers = buffers_pod(2560 * 4, 1600);
        assert!(Pod::from_bytes(&buffers).is_some());
    }
}
