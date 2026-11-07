//! The shutter (`[capture] sound`, 6.17): a discreet click as the picture
//! is taken, at the capture's flash, never when the default output is
//! muted. The sound theme's `screen-capture` event (the freedesktop sound
//! naming), else a short click of our own, played by `pw-play` on
//! PipeWire's default output; whether that output is muted asked of
//! `wpctl` first. Both on a thread of their own, never on the loop. Not
//! for the recording's countdown.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{config::SoundPrograms, programs};

/// The click's rate, mono.
const RATE: u32 = 48_000;

/// Where the programs are, and the PipeWire they talk to.
#[derive(Debug, Clone)]
pub struct Player {
    /// The folder of `wpctl` and `pw-play`, or the `PATH`.
    pub programs: SoundPrograms,
    /// `PIPEWIRE_REMOTE`, when not PipeWire's default.
    pub remote: Option<PathBuf>,
}

impl Player {
    fn command(&self, name: &str) -> Command {
        let program = match &self.programs {
            SoundPrograms::In(dir) => dir.join(name),
            SoundPrograms::Path | SoundPrograms::None => PathBuf::from(name),
        };
        let mut command = Command::new(program);
        crate::programs::default_signals(&mut command);
        if let Some(remote) = &self.remote {
            command.env("PIPEWIRE_REMOTE", remote);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// Plays the click, off the loop, unless the default output is muted.
    pub fn play(self) {
        if self.programs == SoundPrograms::None {
            tracing::debug!("no sound programs: no shutter");
            return;
        }
        programs::on_a_thread("the shutter".to_owned(), move || self.run());
    }

    fn run(&self) {
        let said = self
            .command("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .stdout(Stdio::piped())
            .output();
        match said {
            Ok(out) if out.status.success() => {
                if silent(&String::from_utf8_lossy(&out.stdout)) {
                    tracing::debug!("the default output is muted: no shutter");
                    return;
                }
            }
            Ok(out) => {
                tracing::debug!(status = %out.status, "no default output: no shutter");
                return;
            }
            Err(err) => {
                tracing::debug!(%err, "wpctl is not there: no shutter");
                return;
            }
        }
        let played = match theme_sound() {
            Some(file) => {
                tracing::debug!(file = %file.display(), "the shutter, the sound theme's");
                self.command("pw-play").arg(&file).status().map(drop)
            }
            None => {
                tracing::debug!("the shutter, our own click");
                self.click()
            }
        };
        if let Err(err) = played {
            tracing::debug!(%err, "pw-play cannot play the shutter");
        }
    }

    /// Our own click, raw, through `pw-play`'s input.
    fn click(&self) -> std::io::Result<()> {
        let mut child = self
            .command("pw-play")
            .args([
                "--raw",
                "--format",
                "s16",
                "--rate",
                &RATE.to_string(),
                "--channels",
                "1",
                "-",
            ])
            .stdin(Stdio::piped())
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(&samples())?;
        }
        child.wait().map(drop)
    }
}

/// Whether `wpctl get-volume` says the output is muted, or at nothing:
/// `Volume: 0.40 [MUTED]`.
pub fn silent(said: &str) -> bool {
    if said.contains("[MUTED]") {
        return true;
    }
    said.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<f32>().ok())
        .is_some_and(|v| v <= 0.0)
}

/// The sound theme's `screen-capture`, in the data directories.
fn theme_sound() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        dirs.push(home.into());
    }
    let system = std::env::var_os("XDG_DATA_DIRS")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&system));
    dirs.iter().find_map(|dir| {
        ["oga", "ogg", "wav"].iter().find_map(|ext| {
            let file =
                Path::new(dir).join(format!("sounds/freedesktop/stereo/screen-capture.{ext}"));
            file.is_file().then_some(file)
        })
    })
}

/// The click: 45 ms of a 2.4 kHz tone and a little noise, falling away at
/// once, quiet.
fn samples() -> Vec<u8> {
    let count = RATE as usize * 45 / 1000;
    let mut seed: u32 = 0x9e37_79b9;
    let mut out = Vec::with_capacity(count * 2);
    for i in 0..count {
        let t = i as f32 / RATE as f32;
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let tone = (t * 2400.0 * std::f32::consts::TAU).sin();
        let fall = (-t * 120.0).exp();
        let v = (0.7 * tone + 0.3 * noise) * fall * 0.25;
        out.extend_from_slice(&((v * f32::from(i16::MAX)) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_muted_or_silent_output_has_no_shutter() {
        assert!(silent("Volume: 0.40 [MUTED]\n"));
        assert!(silent("Volume: 0.00\n"));
        assert!(!silent("Volume: 0.40\n"));
    }

    #[test]
    fn the_click_is_short_and_quiet() {
        let samples = samples();
        assert_eq!(samples.len(), 48_000 * 45 / 1000 * 2);
        let loudest = samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| i16::from_le_bytes(*s).unsigned_abs())
            .max()
            .unwrap_or_default();
        assert!(loudest < i16::MAX as u16 / 3);
    }
}
