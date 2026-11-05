//! A microphone's level, for the Settings' meter: `pw-record` (PipeWire's
//! own tool) records the source asked, one channel of 16 kHz, into a pipe
//! read on a thread of its own, which says the peak of each 33 ms, held
//! and falling over 300 ms. It records only while it runs: the Settings
//! start it while their input group is on screen, and it is a capture
//! stream as any other, so the bar's in-use sign lists it (5.12).

use std::{
    io::Read as _,
    process::{Child, Command, Stdio},
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
    child: Child,
}

impl Meter {
    /// Records the source of serial `target`, the stream named `app` (what
    /// the in-use sign says), calling `level` with each tick's level (0 to
    /// 1, on a scale of decibels from -60) when it changes; `None` when
    /// `pw-record` cannot start.
    pub fn start(
        target: u32,
        app: &str,
        mut level: impl FnMut(f32) -> bool + Send + 'static,
    ) -> Option<Self> {
        let props = format!(
            "{{ node.name = \"{NODE}\" node.description = \"{app}\" application.name = \"{app}\" \
             stream.monitor = true }}"
        );
        let child = Command::new("pw-record")
            .args([
                "--target",
                &target.to_string(),
                "-P",
                &props,
                "--raw",
                "--format",
                "s16",
                "--rate",
                &RATE.to_string(),
                "--channels",
                "1",
                "--latency",
                "33ms",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                tracing::warn!(%err, "pw-record cannot start: no level");
                return None;
            }
        };
        let mut out = child.stdout.take()?;
        let started = std::thread::Builder::new()
            .name("sound: meter".to_owned())
            .spawn(move || {
                let samples = RATE * TICK.as_millis() as usize / 1000;
                let mut buffer = vec![0_u8; samples * 2];
                let mut held = 0.0_f32;
                let mut said = -1.0_f32;
                let fall = TICK.as_secs_f32() / FALL.as_secs_f32();
                while out.read_exact(&mut buffer).is_ok() {
                    let peak = peak(&buffer);
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
            });
        if let Err(err) = started {
            tracing::warn!(%err, "cannot read the level");
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        Some(Self { child })
    }

    /// Its process, for the log.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The loudest of 16-bit samples, from 0 to 1.
fn peak(bytes: &[u8]) -> f32 {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)).abs() / 32768.0)
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
        assert!((peak(&samples) - 0.5).abs() < 1e-6);
        assert_eq!(decibels(0.0), 0.0);
        assert!((decibels(1.0) - 1.0).abs() < 1e-6);
        assert!((decibels(0.001) - 0.0).abs() < 1e-6);
        assert!((decibels(0.1) - 2.0 / 3.0).abs() < 1e-3);
    }
}
