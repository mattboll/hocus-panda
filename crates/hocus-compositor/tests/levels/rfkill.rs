//! The radios' rfkill, faked on a Unix socket of the test's own that the
//! Settings open instead of `/dev/rfkill` (`HOCUS_RFKILL`), never the
//! machine's: it says each radio as a client connects, as the kernel does
//! (`RFKILL_OP_ADD`), takes `RFKILL_OP_CHANGE_ALL` by kind, and says each
//! radio that changed to every client (`RFKILL_OP_CHANGE`). What it was
//! asked is recorded.
#![allow(dead_code)]

use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

pub const WLAN: u8 = 1;
pub const BLUETOOTH: u8 = 2;
pub const WWAN: u8 = 5;

const OP_ADD: u8 = 0;
const OP_CHANGE: u8 = 2;
const OP_CHANGE_ALL: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Radio {
    index: u32,
    kind: u8,
    soft: bool,
    hard: bool,
}

impl Radio {
    fn event(self, op: u8) -> [u8; 8] {
        let i = self.index.to_ne_bytes();
        [
            i[0],
            i[1],
            i[2],
            i[3],
            self.kind,
            op,
            u8::from(self.soft),
            u8::from(self.hard),
        ]
    }
}

#[derive(Debug, Default)]
struct State {
    radios: Vec<Radio>,
    clients: Vec<UnixStream>,
    /// What was asked: a kind, blocked or not.
    asked: Vec<(u8, bool)>,
}

type Shared = Arc<Mutex<State>>;

fn state<T>(shared: &Shared, with: impl FnOnce(&mut State) -> T) -> T {
    with(&mut shared.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The fake: its socket, its radios.
pub struct Rfkill {
    pub path: PathBuf,
    shared: Shared,
}

impl Rfkill {
    /// Radios of these kinds, each blocked by software or not, served at
    /// `dir/rfkill`.
    pub fn start(dir: &Path, radios: &[(u8, bool)]) -> anyhow::Result<Self> {
        let path = dir.join("rfkill");
        let listener = UnixListener::bind(&path)?;
        let shared: Shared = Arc::new(Mutex::new(State {
            radios: radios
                .iter()
                .enumerate()
                .map(|(i, (kind, soft))| Radio {
                    index: u32::try_from(i).unwrap_or_default(),
                    kind: *kind,
                    soft: *soft,
                    hard: false,
                })
                .collect(),
            ..State::default()
        }));
        let accepted = shared.clone();
        std::thread::spawn(move || {
            for client in listener.incoming() {
                let Ok(mut client) = client else {
                    return;
                };
                let radios = state(&accepted, |s| s.radios.clone());
                for radio in radios {
                    let _ = client.write_all(&radio.event(OP_ADD));
                }
                let Ok(reader) = client.try_clone() else {
                    continue;
                };
                state(&accepted, |s| s.clients.push(client));
                let shared = accepted.clone();
                std::thread::spawn(move || serve(reader, &shared));
            }
        });
        Ok(Self { path, shared })
    }

    /// Whether `kind`'s radios are blocked by software (`None`: none).
    pub fn soft(&self, kind: u8) -> Option<bool> {
        state(&self.shared, |s| {
            let of: Vec<&Radio> = s.radios.iter().filter(|r| r.kind == kind).collect();
            (!of.is_empty()).then(|| of.iter().all(|r| r.soft))
        })
    }

    /// What was asked, in order: a kind, blocked or not.
    pub fn asked(&self) -> Vec<(u8, bool)> {
        state(&self.shared, |s| s.asked.clone())
    }

    /// A radio of `kind` blocked by its switch (or no longer).
    pub fn set_hard(&self, kind: u8, hard: bool) {
        change(&self.shared, kind, |r| r.hard = hard);
    }
}

/// `kind`'s radios changed by `with`, said to every client.
fn change(shared: &Shared, kind: u8, with: impl Fn(&mut Radio)) {
    state(shared, |s| {
        let mut changed = Vec::new();
        for radio in s.radios.iter_mut().filter(|r| kind == 0 || r.kind == kind) {
            with(radio);
            changed.push(*radio);
        }
        for radio in changed {
            s.clients
                .retain_mut(|c| c.write_all(&radio.event(OP_CHANGE)).is_ok());
        }
    });
}

/// What a client writes, done.
fn serve(mut reader: UnixStream, shared: &Shared) {
    let mut event = [0u8; 8];
    while reader.read_exact(&mut event).is_ok() {
        if event[5] != OP_CHANGE_ALL {
            continue;
        }
        let (kind, soft) = (event[4], event[6] != 0);
        state(shared, |s| s.asked.push((kind, soft)));
        change(shared, kind, |r| r.soft = soft);
    }
}
