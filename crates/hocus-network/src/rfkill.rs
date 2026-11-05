//! The radios' switches (`/dev/rfkill`), for airplane mode (settings.md
//! "Réseau"): which radios there are and whether each is blocked, by
//! software or by a switch, and blocking or unblocking them by kind.
//!
//! The kernel says every radio as the device opens (`RFKILL_OP_ADD`), then
//! each change, whoever makes it (NetworkManager, BlueZ, a hardware
//! airplane key); a thread reads them and tells its owner. Airplane mode is
//! every Wi-Fi, Bluetooth and mobile broadband radio blocked; turning it on
//! blocks them by kind (`RFKILL_OP_CHANGE_ALL`) after writing which kinds
//! were on in a small file, and turning it off unblocks those kinds only
//! (every kind when the file says none). NetworkManager and BlueZ follow
//! rfkill by themselves.
//!
//! The session's user opens `/dev/rfkill` through logind's seat access.
//! Tests give a Unix socket of their own instead (`HOCUS_RFKILL`), which
//! speaks the same events.

use std::{
    io::{Read, Write},
    os::unix::{fs::FileTypeExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
};

/// `enum rfkill_type`.
pub const ALL: u8 = 0;
pub const WLAN: u8 = 1;
pub const BLUETOOTH: u8 = 2;
pub const WWAN: u8 = 5;
/// The kinds airplane mode blocks.
pub const AIRPLANE: [u8; 3] = [WLAN, BLUETOOTH, WWAN];

/// `enum rfkill_operation`.
const OP_ADD: u8 = 0;
const OP_DEL: u8 = 1;
const OP_CHANGE: u8 = 2;
const OP_CHANGE_ALL: u8 = 3;

/// `struct rfkill_event`, as every kernel since 2.6.31 reads and writes it.
const EVENT: usize = 8;

/// A radio as the kernel says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Radio {
    pub index: u32,
    pub kind: u8,
    /// Blocked by software (anyone may unblock it).
    pub soft: bool,
    /// Blocked by a switch or a key: only that unblocks it.
    pub hard: bool,
}

/// Whether the radios of a kind are blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    No,
    /// By software: airplane mode, or the radio turned off.
    Soft,
    /// By a switch or a key.
    Hard,
}

/// The radios there are.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Radios {
    pub radios: Vec<Radio>,
}

impl Radios {
    /// Airplane mode: some radio of [`AIRPLANE`]'s kinds, and every one
    /// blocked.
    pub fn airplane(&self) -> bool {
        let mut radios = self
            .radios
            .iter()
            .filter(|r| AIRPLANE.contains(&r.kind))
            .peekable();
        radios.peek().is_some() && radios.all(|r| r.soft || r.hard)
    }

    /// How `kind`'s radios are blocked: by a switch if one is, else by
    /// software if every one is.
    pub fn block(&self, kind: u8) -> Block {
        let radios: Vec<&Radio> = self.radios.iter().filter(|r| r.kind == kind).collect();
        if radios.iter().any(|r| r.hard) {
            Block::Hard
        } else if !radios.is_empty() && radios.iter().all(|r| r.soft) {
            Block::Soft
        } else {
            Block::No
        }
    }

    /// The kinds of [`AIRPLANE`] with a radio on.
    fn on(&self) -> Vec<u8> {
        AIRPLANE
            .into_iter()
            .filter(|kind| {
                self.radios
                    .iter()
                    .any(|r| r.kind == *kind && !r.soft && !r.hard)
            })
            .collect()
    }

    fn apply(&mut self, event: [u8; EVENT]) {
        let index = u32::from_ne_bytes([event[0], event[1], event[2], event[3]]);
        let radio = Radio {
            index,
            kind: event[4],
            soft: event[6] != 0,
            hard: event[7] != 0,
        };
        match event[5] {
            OP_ADD | OP_CHANGE => match self.radios.iter_mut().find(|r| r.index == index) {
                Some(known) => *known = radio,
                None => self.radios.push(radio),
            },
            OP_DEL => self.radios.retain(|r| r.index != index),
            _ => {}
        }
    }
}

/// What the owner asks of the radios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Airplane mode on, or off (what was on before comes back).
    Airplane(bool),
    /// `kind`'s radios unblocked: the Wi-Fi turned on in airplane mode
    /// turns on alone.
    Unblock(u8),
}

/// The device the radios are read from: `HOCUS_RFKILL`, else
/// `/dev/rfkill`.
pub fn device() -> PathBuf {
    std::env::var_os("HOCUS_RFKILL").map_or_else(|| PathBuf::from("/dev/rfkill"), PathBuf::from)
}

/// The device opened, for reading and writing.
enum Opened {
    File(std::fs::File),
    Socket(UnixStream),
}

impl Opened {
    fn open(path: &Path) -> std::io::Result<Self> {
        let socket = std::fs::metadata(path).is_ok_and(|m| m.file_type().is_socket());
        if socket {
            UnixStream::connect(path).map(Self::Socket)
        } else {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map(Self::File)
        }
    }

    fn try_clone(&self) -> std::io::Result<Self> {
        match self {
            Self::File(f) => f.try_clone().map(Self::File),
            Self::Socket(s) => s.try_clone().map(Self::Socket),
        }
    }

    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        match self {
            Self::File(f) => {
                let flags = rustix::fs::fcntl_getfl(f)?;
                let flags = if on {
                    flags | rustix::fs::OFlags::NONBLOCK
                } else {
                    flags - rustix::fs::OFlags::NONBLOCK
                };
                rustix::fs::fcntl_setfl(f, flags)?;
                Ok(())
            }
            Self::Socket(s) => s.set_nonblocking(on),
        }
    }

    fn read_event(&mut self) -> std::io::Result<[u8; EVENT]> {
        let mut event = [0u8; EVENT];
        match self {
            Self::File(f) => f.read_exact(&mut event)?,
            Self::Socket(s) => s.read_exact(&mut event)?,
        }
        Ok(event)
    }

    fn write_event(&mut self, event: [u8; EVENT]) -> std::io::Result<()> {
        match self {
            Self::File(f) => f.write_all(&event),
            Self::Socket(s) => s.write_all(&event),
        }
    }
}

/// Reads the radios at `path` on a thread of its own and tells `on` each
/// time they change (`None`: they cannot be read); `asks` are done on
/// another, `memory` keeping which kinds were on before airplane mode.
pub fn start(
    path: PathBuf,
    memory: Option<PathBuf>,
    asks: Receiver<Ask>,
    mut on: impl FnMut(Option<Radios>) -> bool + Send + 'static,
) {
    let opened = Opened::open(&path).and_then(|reader| Ok((reader.try_clone()?, reader)));
    let (writer, mut reader) = match opened {
        Ok(both) => both,
        Err(err) => {
            tracing::info!(%err, path = %path.display(), "the radios do not open: no airplane mode");
            on(None);
            return;
        }
    };
    let radios = std::sync::Arc::new(std::sync::Mutex::new(Radios::default()));
    let read = radios.clone();
    let started = std::thread::Builder::new()
        .name("rfkill".to_owned())
        .spawn(move || {
            // The radios there are, said as the device opens: read whole,
            // then said once; then each change as it comes.
            let mut now = Radios::default();
            let first = reader.set_nonblocking(true).and_then(|()| {
                loop {
                    match reader.read_event() {
                        Ok(event) => now.apply(event),
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(err) => return Err(err),
                    }
                }
                reader.set_nonblocking(false)
            });
            if let Err(err) = first {
                tracing::warn!(%err, "the radios do not read");
                on(None);
                return;
            }
            loop {
                *read
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = now.clone();
                if !on(Some(now.clone())) {
                    return;
                }
                match reader.read_event() {
                    Ok(event) => now.apply(event),
                    Err(err) => {
                        tracing::warn!(%err, "the radios are no longer read");
                        on(None);
                        return;
                    }
                }
            }
        })
        .and_then(|_| {
            std::thread::Builder::new()
                .name("rfkill: set".to_owned())
                .spawn(move || ask(writer, &radios, memory.as_deref(), &asks))
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot read the radios");
    }
}

/// Does what is asked until the owner goes.
fn ask(
    mut writer: Opened,
    radios: &std::sync::Mutex<Radios>,
    memory: Option<&Path>,
    asks: &Receiver<Ask>,
) {
    while let Ok(ask) = asks.recv() {
        let now = radios
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let changes: Vec<(u8, bool)> = match ask {
            Ask::Airplane(true) => {
                remember(memory, &now.on());
                AIRPLANE.into_iter().map(|kind| (kind, true)).collect()
            }
            Ask::Airplane(false) => {
                let was = recall(memory);
                let kinds = if was.is_empty() {
                    AIRPLANE.to_vec()
                } else {
                    was
                };
                kinds.into_iter().map(|kind| (kind, false)).collect()
            }
            Ask::Unblock(kind) => vec![(kind, false)],
        };
        tracing::info!(?ask, ?changes, "the radios switched");
        for (kind, block) in changes {
            let event = [0, 0, 0, 0, kind, OP_CHANGE_ALL, u8::from(block), 0];
            if let Err(err) = writer.write_event(event) {
                tracing::warn!(%err, kind, "the radios did not switch");
            }
        }
    }
}

/// Which kinds were on, kept in `memory` as their names.
fn remember(memory: Option<&Path>, on: &[u8]) {
    let Some(memory) = memory else {
        return;
    };
    let names: String = on.iter().map(|kind| format!("{}\n", name(*kind))).collect();
    let written = memory
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(memory, names));
    if let Err(err) = written {
        tracing::warn!(%err, "what was on before airplane mode is not kept");
    }
}

/// The kinds `memory` says were on, and the file gone.
fn recall(memory: Option<&Path>) -> Vec<u8> {
    let Some(memory) = memory else {
        return Vec::new();
    };
    let kinds = std::fs::read_to_string(memory)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| AIRPLANE.into_iter().find(|k| name(*k) == line.trim()))
        .collect();
    let _ = std::fs::remove_file(memory);
    kinds
}

fn name(kind: u8) -> &'static str {
    match kind {
        WLAN => "wlan",
        BLUETOOTH => "bluetooth",
        WWAN => "wwan",
        ALL => "all",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(index: u32, kind: u8, op: u8, soft: bool, hard: bool) -> [u8; EVENT] {
        let i = index.to_ne_bytes();
        [
            i[0],
            i[1],
            i[2],
            i[3],
            kind,
            op,
            u8::from(soft),
            u8::from(hard),
        ]
    }

    #[test]
    fn airplane_mode_is_every_radio_blocked() {
        let mut radios = Radios::default();
        assert!(!radios.airplane(), "no radio, no airplane mode");
        radios.apply(event(0, WLAN, OP_ADD, false, false));
        radios.apply(event(1, BLUETOOTH, OP_ADD, true, false));
        assert!(!radios.airplane());
        assert_eq!(radios.on(), [WLAN]);
        assert_eq!(radios.block(BLUETOOTH), Block::Soft);
        radios.apply(event(0, WLAN, OP_CHANGE, true, false));
        assert!(radios.airplane());
        radios.apply(event(0, WLAN, OP_CHANGE, false, true));
        assert_eq!(radios.block(WLAN), Block::Hard);
        radios.apply(event(1, BLUETOOTH, OP_DEL, false, false));
        assert_eq!(radios.radios.len(), 1);
        assert_eq!(radios.block(BLUETOOTH), Block::No);
    }

    #[test]
    fn what_was_on_is_kept_then_given_back_once() {
        let dir = std::env::temp_dir().join(format!("hocus-rfkill-{}", std::process::id()));
        let memory = dir.join("airplane");
        remember(Some(&memory), &[WLAN, WWAN]);
        assert_eq!(
            std::fs::read_to_string(&memory).unwrap_or_default(),
            "wlan\nwwan\n"
        );
        assert_eq!(recall(Some(&memory)), [WLAN, WWAN]);
        assert!(recall(Some(&memory)).is_empty(), "read once");
        let _ = std::fs::remove_dir(&dir);
    }
}
