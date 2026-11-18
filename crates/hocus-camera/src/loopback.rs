//! The Hocuspanda camera on V4L2 too (#619, docs/design/camera.md): the
//! same frames written to a v4l2loopback device labelled "Hocuspanda", for
//! the applications that read cameras only from `/dev/video*` (Firefox and
//! Chrome without their PipeWire setting, Slack, Teams, Discord).
//!
//! - **The device** is found by its label, never by its number: the
//!   `name` of each `/sys/class/video4linux` entry (`HOCUS_VIDEO4LINUX`
//!   stands for that directory), its node in `/dev` (`HOCUS_VIDEO_DEVICES`)
//!   by its `uevent`'s `DEVNAME`. Only that node is opened, and kept only
//!   if `VIDIOC_QUERYCAP` says it is v4l2loopback's and labelled so: no
//!   other `/dev/video*` is ever opened. The package configures the module
//!   (`packaging/modprobe-hocuspanda-camera.conf`); it never loads it.
//! - **Its format**: YUYV 4:2:2, 1280 × 720, 30 frames a second, BT.601's
//!   limited range: what every UVC webcam sends, so what every reader
//!   takes (Firefox's and Chrome's WebRTC, Electron's applications).
//! - **Written on a thread of its own**, from the frames the PipeWire
//!   source gets ([`Writer::send`]), converted there. With nothing new for
//!   half a second, the last frame again: the still (a dark frame, with
//!   the banner) while nobody reads, so a reader starting gets a picture
//!   at once, and with `exclusive_caps` the device shows as a camera at
//!   all only once written.
//! - **Its readers**: v4l2loopback's client-usage event (from 0.12.5),
//!   which counts the readers streaming from the device; on an older one,
//!   the processes holding it open (`/proc`, at most once a second), but
//!   this one and PipeWire's (whose own node of the device reading it is
//!   a stream the graph shows). The main loop is told when that changes:
//!   it reads the real camera while the source or the device is read.
//!
//! The V4L2 structures are the kernel's user-space interface
//! (`linux/videodev2.h`), declared here as they are laid out on a 64-bit
//! system; their sizes are checked at build time.

use std::{
    io,
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
    time::{Duration, Instant},
};

use hocus_link::camera::{CAMERA_DESCRIPTION, CAMERA_FPS, CAMERA_SIZE};
use rustix::{
    fs::{Mode, OFlags},
    ioctl::{self, Getter, Opcode, Setter, Updater},
};

use crate::convert::{self, Output};

/// The device's label (`card_label`), and its name in sysfs.
pub const LABEL: &str = CAMERA_DESCRIPTION;
/// What `VIDIOC_QUERYCAP` says v4l2loopback's devices' driver is.
pub const DRIVER: &str = "v4l2 loopback";
/// The last frame is written again after this long without a new one.
const REPEAT: Duration = Duration::from_millis(500);
/// `/proc` is read at most this often, where the device has no event.
const PROC_EVERY: Duration = Duration::from_secs(1);
/// The programs whose holding of the device is a PipeWire stream's.
const PIPEWIRE: [&str; 3] = ["pipewire", "wireplumber", "pipewire-media-"];

/// The directory standing for `/sys/class/video4linux`.
pub fn sysfs_dir() -> PathBuf {
    std::env::var_os("HOCUS_VIDEO4LINUX")
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/sys/class/video4linux"), PathBuf::from)
}

/// The directory of the devices' nodes: `/dev`.
pub fn devices_dir() -> PathBuf {
    std::env::var_os("HOCUS_VIDEO_DEVICES")
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/dev"), PathBuf::from)
}

/// The node of the device labelled [`LABEL`] in `sysfs`, under `devices`:
/// read from sysfs alone, nothing opened.
pub fn find(sysfs: &Path, devices: &Path) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(sysfs).ok()?.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries.iter().find_map(|entry| {
        let dir = entry.path();
        let name = std::fs::read_to_string(dir.join("name")).ok()?;
        if name.trim_end_matches(['\n', '\0']) != LABEL {
            return None;
        }
        let devname = std::fs::read_to_string(dir.join("uevent"))
            .ok()
            .and_then(|uevent| {
                uevent
                    .lines()
                    .find_map(|l| l.strip_prefix("DEVNAME=").map(str::to_owned))
            })
            .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        // A name in the devices' directory, never a way out of it.
        if devname.is_empty() || devname.starts_with('/') || devname.split('/').any(|p| p == "..") {
            return None;
        }
        Some(devices.join(devname))
    })
}

/// Whether the device is there (the shell's help says what works).
pub fn present() -> bool {
    find(&sysfs_dir(), &devices_dir()).is_some()
}

// The kernel's structures (videodev2.h), as laid out on 64-bit Linux.

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct PixFormat {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    private: u32,
    flags: u32,
    ycbcr_enc: u32,
    quantization: u32,
    xfer_func: u32,
}

/// `v4l2_format`'s union: its largest member is 200 bytes, and one holds
/// pointers (`v4l2_window`), which align it as a pointer.
#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
#[derive(Clone, Copy)]
union FormatUnion {
    pix: PixFormat,
    raw: [u8; 200],
    _align: *mut core::ffi::c_void,
}

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
struct Format {
    kind: u32,
    fmt: FormatUnion,
}

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
#[derive(Default)]
struct OutputParm {
    capability: u32,
    outputmode: u32,
    timeperframe: [u32; 2],
    extendedmode: u32,
    writebuffers: u32,
    reserved: [u32; 4],
}

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
struct StreamParm {
    kind: u32,
    output: OutputParm,
    rest: [u8; 200 - 40],
}

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
#[derive(Debug, Default)]
struct EventSubscription {
    kind: u32,
    id: u32,
    flags: u32,
    reserved: [u32; 5],
}

/// `v4l2_event`'s union holds an `__s64`: aligned as one.
#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct EventData([u8; 64]);

#[allow(dead_code, reason = "the kernel's layout, all of it")]
#[repr(C)]
struct Event {
    kind: u32,
    data: EventData,
    pending: u32,
    sequence: u32,
    timestamp: [i64; 2],
    id: u32,
    reserved: [u32; 8],
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<Capability>() == 104);
    assert!(size_of::<Format>() == 208);
    assert!(size_of::<StreamParm>() == 204);
    assert!(size_of::<EventSubscription>() == 32);
    assert!(size_of::<Event>() == 136);
};

const QUERYCAP: Opcode = ioctl::opcode::read::<Capability>(b'V', 0);
const S_FMT: Opcode = ioctl::opcode::read_write::<Format>(b'V', 5);
const S_PARM: Opcode = ioctl::opcode::read_write::<StreamParm>(b'V', 22);
const DQEVENT: Opcode = ioctl::opcode::read::<Event>(b'V', 89);
const SUBSCRIBE_EVENT: Opcode = ioctl::opcode::write::<EventSubscription>(b'V', 90);

const BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
const FIELD_NONE: u32 = 1;
const COLORSPACE_SMPTE170M: u32 = 1;
const YCBCR_ENC_601: u32 = 1;
const QUANTIZATION_LIM_RANGE: u32 = 2;
const YUYV: u32 = u32::from_le_bytes(*b"YUYV");
/// `V4L2_EVENT_PRIVATE_START`.
const EVENT_PRIVATE_START: u32 = 0x0800_0000;
/// v4l2loopback's client-usage event (`V4L2_EVENT_PRI_CLIENT_USAGE`): its
/// data's first `__u32` is how many readers stream from the device.
pub const EVENT_CLIENT_USAGE: u32 = EVENT_PRIVATE_START + 0x08E0_0000 + 1;

/// A NUL-ended string of the kernel's.
fn text(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(bytes.get(..end).unwrap_or_default()).unwrap_or_default()
}

/// The readers an event says stream from the device, if it is the
/// client-usage event.
pub fn usage(kind: u32, data: &[u8; 64]) -> Option<u32> {
    if kind != EVENT_CLIENT_USAGE {
        return None;
    }
    let (count, _) = data.split_first_chunk::<4>()?;
    Some(u32::from_ne_bytes(*count))
}

/// How the device's readers are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detect {
    /// Its client-usage event.
    Events,
    /// The processes holding it open, but this one and PipeWire's.
    Holders,
}

/// Whether the device is read, from its readers' counts: a change said
/// once.
#[derive(Debug, Default)]
pub struct Readers {
    read: bool,
}

impl Readers {
    /// `count` readers now: whether it is read, when that changed.
    pub fn seen(&mut self, count: u32) -> Option<bool> {
        let read = count > 0;
        (read != self.read).then(|| {
            self.read = read;
            read
        })
    }
}

/// How many processes in `proc` (standing for `/proc`) hold `device`
/// open: not `me`, nor PipeWire's.
pub fn holders(proc: &Path, device: &Path, me: u32) -> u32 {
    let Ok(entries) = std::fs::read_dir(proc) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let dir = entry.path();
        let Ok(fds) = std::fs::read_dir(dir.join("fd")) else {
            continue;
        };
        let holds = fds
            .flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .any(|target| target == device);
        if !holds {
            continue;
        }
        let program = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        if PIPEWIRE.iter().any(|p| program.trim().starts_with(p)) {
            continue;
        }
        count += 1;
    }
    count
}

/// The device being written, from the main loop's side.
#[derive(Debug)]
pub struct Writer {
    frames: SyncSender<Vec<u8>>,
    spare: Receiver<Vec<u8>>,
    pub device: PathBuf,
}

impl Writer {
    /// `frame` (B, G, R, x, 1280 × 720) to write; dropped while the
    /// thread is still writing the one before.
    pub fn send(&self, frame: &[u8]) {
        let mut buffer = self.spare.try_recv().unwrap_or_default();
        buffer.clear();
        buffer.extend_from_slice(frame);
        let _ = self.frames.try_send(buffer);
    }
}

/// Opens the device labelled "Hocuspanda", sets its format and starts its
/// thread, which tells `read` whether it is read as that changes. `None`:
/// there is none.
#[allow(unsafe_code)]
pub fn start(read: impl Fn(bool) + Send + 'static) -> io::Result<Option<Writer>> {
    let Some(device) = find(&sysfs_dir(), &devices_dir()) else {
        return Ok(None);
    };
    if cfg!(not(target_pointer_width = "64")) {
        return Err(io::Error::other("V4L2 is written on 64-bit systems only"));
    }
    let fd = rustix::fs::open(
        &device,
        OFlags::RDWR | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    // SAFETY: QUERYCAP fills a `v4l2_capability`, which `Capability` is
    // laid out as (its size checked above, and in the opcode).
    let caps = unsafe { ioctl::ioctl(&fd, Getter::<QUERYCAP, Capability>::new()) }?;
    let (driver, card) = (text(&caps.driver), text(&caps.card));
    if driver != DRIVER || card != LABEL {
        return Err(io::Error::other(format!(
            "{} is {driver:?}'s {card:?}, not v4l2loopback's {LABEL:?}",
            device.display()
        )));
    }
    let (width, height) = CAMERA_SIZE;
    let (stride, size) = Output::Yuy2.layout();
    let mut format = Format {
        kind: BUF_TYPE_VIDEO_OUTPUT,
        fmt: FormatUnion { raw: [0; 200] },
    };
    format.fmt.pix = PixFormat {
        width,
        height,
        pixelformat: YUYV,
        field: FIELD_NONE,
        bytesperline: u32::try_from(stride).unwrap_or(u32::MAX),
        sizeimage: u32::try_from(size).unwrap_or(u32::MAX),
        colorspace: COLORSPACE_SMPTE170M,
        ycbcr_enc: YCBCR_ENC_601,
        quantization: QUANTIZATION_LIM_RANGE,
        ..PixFormat::default()
    };
    // SAFETY: S_FMT reads and writes a `v4l2_format`, which `Format` is
    // laid out as; `format` lives through the call.
    unsafe { ioctl::ioctl(&fd, Updater::<S_FMT, Format>::new(&mut format)) }?;
    // SAFETY: `pix` was written above, and the kernel writes a
    // `v4l2_pix_format` back for an output's type.
    let agreed = unsafe { format.fmt.pix };
    if (agreed.width, agreed.height, agreed.pixelformat) != (width, height, YUYV) {
        return Err(io::Error::other(format!(
            "{} takes {}×{} {:?}, not YUYV 1280×720",
            device.display(),
            agreed.width,
            agreed.height,
            agreed.pixelformat.to_le_bytes()
        )));
    }
    let mut parm = StreamParm {
        kind: BUF_TYPE_VIDEO_OUTPUT,
        output: OutputParm {
            timeperframe: [1, CAMERA_FPS],
            ..OutputParm::default()
        },
        rest: [0; 160],
    };
    // SAFETY: S_PARM reads and writes a `v4l2_streamparm`, which
    // `StreamParm` is laid out as; `parm` lives through the call.
    if let Err(err) = unsafe { ioctl::ioctl(&fd, Updater::<S_PARM, StreamParm>::new(&mut parm)) } {
        tracing::debug!(%err, "the V4L2 device's frame rate not set");
    }
    let subscription = EventSubscription {
        kind: EVENT_CLIENT_USAGE,
        ..EventSubscription::default()
    };
    // SAFETY: SUBSCRIBE_EVENT reads a `v4l2_event_subscription`, which
    // `EventSubscription` is laid out as.
    let detect = match unsafe {
        ioctl::ioctl(
            &fd,
            Setter::<SUBSCRIBE_EVENT, EventSubscription>::new(subscription),
        )
    } {
        Ok(()) => Detect::Events,
        Err(err) => {
            tracing::info!(%err, "no client-usage event (v4l2loopback before 0.12.5): its readers read from /proc");
            Detect::Holders
        }
    };
    let (frames, from_loop) = mpsc::sync_channel::<Vec<u8>>(1);
    let (to_loop, spare) = mpsc::channel::<Vec<u8>>();
    let at = device.clone();
    std::thread::Builder::new()
        .name("camera: v4l2".to_owned())
        .spawn(move || run(&fd, &at, detect, &from_loop, &to_loop, &read))?;
    tracing::info!(device = %device.display(), ?detect, "the V4L2 device is written");
    Ok(Some(Writer {
        frames,
        spare,
        device,
    }))
}

/// The pending client-usage events' last count, if any.
#[allow(unsafe_code)]
fn drain(fd: &OwnedFd) -> Option<u32> {
    let mut last = None;
    loop {
        // SAFETY: DQEVENT fills a `v4l2_event`, which `Event` is laid out
        // as; on a non-blocking descriptor it fails at once when none is
        // pending.
        match unsafe { ioctl::ioctl(fd, Getter::<DQEVENT, Event>::new()) } {
            Ok(event) => {
                if let Some(count) = usage(event.kind, &event.data.0) {
                    last = Some(count);
                }
            }
            Err(_) => return last,
        }
    }
}

/// The thread: each frame converted and written, the last one again
/// after [`REPEAT`] without one, its readers followed; until the main
/// loop lets go of its [`Writer`].
fn run(
    fd: &OwnedFd,
    device: &Path,
    detect: Detect,
    frames: &Receiver<Vec<u8>>,
    spare: &Sender<Vec<u8>>,
    told: &dyn Fn(bool),
) {
    let me = std::process::id();
    let mut yuyv = vec![0u8; Output::Yuy2.layout().1];
    let mut written = false;
    let mut failing = false;
    let mut readers = Readers::default();
    let mut read_at: Option<Instant> = None;
    loop {
        let frame = match frames.recv_timeout(REPEAT) {
            Ok(frame) => Some(frame),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if let Some(frame) = frame {
            written = convert::write(&frame, Output::Yuy2, &mut yuyv) > 0;
            let _ = spare.send(frame);
        }
        if written {
            match rustix::io::write(fd, &yuyv) {
                Ok(_) => failing = false,
                Err(err) if !failing => {
                    tracing::warn!(%err, device = %device.display(), "a frame not written to the V4L2 device");
                    failing = true;
                }
                Err(_) => {}
            }
        }
        let count = match detect {
            Detect::Events => drain(fd),
            Detect::Holders => {
                let now = Instant::now();
                read_at
                    .is_none_or(|at| now.duration_since(at) >= PROC_EVERY)
                    .then(|| {
                        read_at = Some(now);
                        holders(Path::new("/proc"), device, me)
                    })
            }
        };
        if let Some(read) = count.and_then(|c| readers.seen(c)) {
            tracing::info!(read, "the V4L2 device");
            told(read);
        }
    }
    if readers.read {
        told(false);
    }
    tracing::debug!("the V4L2 device let go");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_is_found_by_its_label_never_its_number() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let sysfs = dir.path().join("video4linux");
        let devices = dir.path().join("dev");
        let entry = |name: &str, label: &str, devname: Option<&str>| -> io::Result<()> {
            let at = sysfs.join(name);
            std::fs::create_dir_all(&at)?;
            std::fs::write(at.join("name"), format!("{label}\n"))?;
            if let Some(devname) = devname {
                std::fs::write(
                    at.join("uevent"),
                    format!("MAJOR=81\nMINOR=3\nDEVNAME={devname}\n"),
                )?;
            }
            Ok(())
        };
        // No directory, then the laptop's camera alone: none.
        assert_eq!(find(&sysfs, &devices), None);
        entry("video0", "Integrated Camera: Integrated C", Some("video0"))?;
        entry("video1", "Integrated Camera: Integrated C", Some("video1"))?;
        assert_eq!(find(&sysfs, &devices), None);
        // Another program's loopback, then ours, numbered as it may be.
        entry("video10", "OBS Virtual Camera", Some("video10"))?;
        entry("video42", "Hocuspanda", Some("video42"))?;
        assert_eq!(find(&sysfs, &devices), Some(devices.join("video42")));
        // Its node as its uevent names it; else as its entry is named.
        std::fs::remove_file(sysfs.join("video42/uevent"))?;
        assert_eq!(find(&sysfs, &devices), Some(devices.join("video42")));
        std::fs::write(sysfs.join("video42/uevent"), "DEVNAME=v4l/cam\n")?;
        assert_eq!(find(&sysfs, &devices), Some(devices.join("v4l/cam")));
        // Never a node out of the devices' directory.
        std::fs::write(sysfs.join("video42/uevent"), "DEVNAME=../etc/passwd\n")?;
        assert_eq!(find(&sysfs, &devices), None);
        Ok(())
    }

    #[test]
    fn its_readers_are_the_client_usage_event_s_count() {
        let mut data = [0u8; 64];
        data[..4].copy_from_slice(&2u32.to_ne_bytes());
        assert_eq!(usage(EVENT_CLIENT_USAGE, &data), Some(2));
        // v4l2loopback's id: the private range's start, its offset, 1.
        assert_eq!(EVENT_CLIENT_USAGE, 0x10E0_0001);
        // Another event says nothing of its readers.
        assert_eq!(usage(EVENT_PRIVATE_START, &data), None);
        let mut readers = Readers::default();
        assert_eq!(readers.seen(0), None);
        assert_eq!(readers.seen(1), Some(true));
        assert_eq!(readers.seen(2), None);
        assert_eq!(readers.seen(0), Some(false));
        assert_eq!(readers.seen(0), None);
    }

    #[test]
    fn a_holder_of_the_device_reads_it_but_this_process_and_pipewire() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let proc = dir.path().join("proc");
        let device = dir.path().join("dev/video42");
        let other = dir.path().join("dev/video0");
        let process = |pid: u32, comm: &str, holds: &Path| -> io::Result<()> {
            let fd = proc.join(pid.to_string()).join("fd");
            std::fs::create_dir_all(&fd)?;
            std::fs::write(proc.join(pid.to_string()).join("comm"), format!("{comm}\n"))?;
            std::os::unix::fs::symlink(holds, fd.join("7"))
        };
        assert_eq!(holders(&proc, &device, 10), 0);
        process(10, "hocus-camera", &device)?;
        process(11, "wireplumber", &device)?;
        process(12, "pipewire", &device)?;
        process(13, "firefox", &other)?;
        assert_eq!(holders(&proc, &device, 10), 0);
        process(14, "slack", &device)?;
        process(15, "chrome", &device)?;
        assert_eq!(holders(&proc, &device, 10), 2);
        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn the_opcodes_are_videodev2_s() {
        assert_eq!(u64::from(QUERYCAP), 0x8068_5600);
        assert_eq!(u64::from(S_FMT), 0xc0d0_5605);
        assert_eq!(u64::from(S_PARM), 0xc0cc_5616);
        assert_eq!(u64::from(DQEVENT), 0x8088_5659);
        assert_eq!(u64::from(SUBSCRIBE_EVENT), 0x4020_565a);
        assert_eq!(YUYV, 0x5659_5559);
    }
}
