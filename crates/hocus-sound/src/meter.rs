//! A microphone's level, for the Settings' meter: `pw-record` (PipeWire's
//! own tool) records the source asked, one channel of 16 kHz, into a pipe
//! read on a thread of its own, which says the peak of each 33 ms, held
//! and falling over 300 ms. It records only while it runs: the Settings
//! start it while their input group is on screen, and it is a capture
//! stream as any other, so the bar's in-use sign lists it (5.12).
//!
//! The stream's name and application are given in `PIPEWIRE_PROPS`, which
//! every PipeWire takes (1.0's `pw-record` ignores `-P`'s); its samples
//! come raw where `pw-record` has `--raw` (1.2 on), else in the container
//! it writes to a pipe, whose header is skipped.

use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// The meter's node name: its streams are told from the applications'.
pub const NODE: &str = "hocus-settings-meter";

const RATE: usize = 16_000;
/// A tick: ~30 levels a second.
pub const TICK: Duration = Duration::from_millis(33);
/// A peak falls to nothing over this long.
const FALL: Duration = Duration::from_millis(300);

/// A meter recording: stopped when dropped (its own `pw-record`, by its
/// process).
#[derive(Debug)]
pub struct Meter {
    child: Arc<Mutex<Option<Child>>>,
    stopped: Arc<AtomicBool>,
}

impl Meter {
    /// Records the source of serial `target`, the stream named `app` (what
    /// the in-use sign says), calling `level` with each tick's level (0 to
    /// 1, on a scale of decibels from -60) when it changes. All of it on a
    /// thread of its own: `pw-record` started there, never waited on by
    /// the caller.
    pub fn start(target: u32, app: &str, level: impl FnMut(f32) -> bool + Send + 'static) -> Self {
        let child: Arc<Mutex<Option<Child>>> = Arc::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let props = format!(
            "{{ node.name = \"{NODE}\" node.description = \"{app}\" application.name = \"{app}\" \
             stream.monitor = true }}"
        );
        let (kept, stop) = (child.clone(), stopped.clone());
        let started = std::thread::Builder::new()
            .name("sound: meter".to_owned())
            .spawn(move || record(target, &props, &kept, &stop, level));
        if let Err(err) = started {
            tracing::warn!(%err, "cannot read the level");
        }
        Self { child, stopped }
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Whether this `pw-record` writes raw samples (`--raw`), asked once.
fn raw() -> bool {
    static RAW: OnceLock<bool> = OnceLock::new();
    *RAW.get_or_init(|| {
        Command::new("pw-record")
            .arg("--help")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("--raw"))
    })
}

/// The meter's thread: `pw-record` started and read until it ends or the
/// meter is dropped.
fn record(
    target: u32,
    props: &str,
    kept: &Mutex<Option<Child>>,
    stopped: &AtomicBool,
    mut level: impl FnMut(f32) -> bool,
) {
    let raw = raw();
    let mut command = Command::new("pw-record");
    command.env("PIPEWIRE_PROPS", props).args([
        "--target",
        &target.to_string(),
        "--format",
        "s16",
        "--rate",
        &RATE.to_string(),
        "--channels",
        "1",
        "--latency",
        "33ms",
    ]);
    if raw {
        command.arg("--raw");
    }
    let spawned = command
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(%err, "pw-record cannot start: no level");
            return;
        }
    };
    let Some(mut out) = child.stdout.take() else {
        return;
    };
    tracing::debug!(pid = child.id(), raw, "the meter's pw-record");
    {
        let mut slot = kept.lock().unwrap_or_else(PoisonError::into_inner);
        if stopped.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        *slot = Some(child);
    }
    let big_endian = if raw {
        false
    } else {
        match skip_header(&mut out) {
            Some(big_endian) => big_endian,
            None => return,
        }
    };
    let samples = RATE * TICK.as_millis() as usize / 1000;
    let mut buffer = vec![0_u8; samples * 2];
    let mut held = 0.0_f32;
    let mut said = -1.0_f32;
    let fall = TICK.as_secs_f32() / FALL.as_secs_f32();
    while out.read_exact(&mut buffer).is_ok() {
        let peak = peak(&buffer, big_endian);
        held = decibels(peak).max(held - fall).max(0.0);
        // Said to a hundredth: silence says nothing more.
        let now = (held * 100.0).round() / 100.0;
        if now != said {
            said = now;
            if !level(now) {
                return;
            }
        }
    }
}

/// A container's header read past: a WAV's up to its `data` chunk, an
/// AU's (`.snd`, big-endian samples) up to its data's offset; whether the
/// samples are big-endian. `None`: the pipe ended.
fn skip_header(out: &mut impl Read) -> Option<bool> {
    let mut magic = [0_u8; 4];
    out.read_exact(&mut magic).ok()?;
    match &magic {
        b"RIFF" => {
            // The RIFF size and "WAVE", then chunks until "data".
            let mut skip = [0_u8; 8];
            out.read_exact(&mut skip).ok()?;
            loop {
                let mut chunk = [0_u8; 8];
                out.read_exact(&mut chunk).ok()?;
                if &chunk[..4] == b"data" {
                    return Some(false);
                }
                let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                let size = u64::from(size) + u64::from(size % 2);
                std::io::copy(&mut out.by_ref().take(size), &mut std::io::sink()).ok()?;
            }
        }
        b".snd" => {
            let mut offset = [0_u8; 4];
            out.read_exact(&mut offset).ok()?;
            let offset = u64::from(u32::from_be_bytes(offset)).saturating_sub(8);
            std::io::copy(&mut out.by_ref().take(offset), &mut std::io::sink()).ok()?;
            Some(true)
        }
        // Raw after all: these were samples.
        _ => Some(false),
    }
}

/// The loudest of 16-bit samples, from 0 to 1.
fn peak(bytes: &[u8], big_endian: bool) -> f32 {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            let sample = if big_endian {
                i16::from_be_bytes(*b)
            } else {
                i16::from_le_bytes(*b)
            };
            f32::from(sample).abs() / 32768.0
        })
        .fold(0.0, f32::max)
}

/// A peak from 0 to 1 on a scale of decibels: -60 dB and below are 0.
fn decibels(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_on_the_meters_scale() {
        let samples: Vec<u8> = [0_i16, -16384, 8000]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        assert!((peak(&samples, false) - 0.5).abs() < 1e-6);
        let big: Vec<u8> = [0_i16, 16384]
            .iter()
            .flat_map(|s| s.to_be_bytes())
            .collect();
        assert!((peak(&big, true) - 0.5).abs() < 1e-6);
        assert_eq!(decibels(0.0), 0.0);
        assert!((decibels(1.0) - 1.0).abs() < 1e-6);
        assert!((decibels(0.001) - 0.0).abs() < 1e-6);
        assert!((decibels(0.1) - 2.0 / 3.0).abs() < 1e-3);
    }

    #[test]
    fn a_containers_header_is_read_past() {
        // A WAV: "fmt " then "data".
        let mut wav = b"RIFF\0\0\0\0WAVEfmt \x04\0\0\0abcddata\0\0\0\0".to_vec();
        wav.extend_from_slice(&[1, 2]);
        let mut reader = wav.as_slice();
        assert_eq!(skip_header(&mut reader), Some(false));
        assert_eq!(reader, [1, 2]);
        // An AU: its data 28 bytes in, big-endian.
        let mut au = b".snd\0\0\0\x1c".to_vec();
        au.extend_from_slice(&[0; 20]);
        au.extend_from_slice(&[3, 4]);
        let mut reader = au.as_slice();
        assert_eq!(skip_header(&mut reader), Some(true));
        assert_eq!(reader, [3, 4]);
    }
}
