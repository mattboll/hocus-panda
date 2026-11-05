//! "Tester": a sound on the left, then on the right, on one output, with
//! `pw-play`: the freedesktop sound theme's `audio-channel-front-left` and
//! `-right` when it is installed, else a short tone of our own, panned.
//! A mono output plays it once, in the middle. On a thread of its own.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Which side plays now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Middle,
}

/// Each side lasts this long, whatever its sound's length.
pub const EACH: Duration = Duration::from_millis(900);
const RATE: u32 = 48_000;

/// Plays the test on the output of serial `target` (both sides, or the
/// middle when `mono`), calling `side` as each starts and with `None`
/// once done.
pub fn play(target: u32, mono: bool, side: impl FnMut(Option<Side>) + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("sound: test".to_owned())
        .spawn(move || run(target, mono, side));
    if let Err(err) = started {
        tracing::warn!(%err, "cannot play the test");
    }
}

fn run(target: u32, mono: bool, mut side: impl FnMut(Option<Side>)) {
    let sides: &[Side] = if mono {
        &[Side::Middle]
    } else {
        &[Side::Left, Side::Right]
    };
    for which in sides {
        let start = Instant::now();
        side(Some(*which));
        let played = match theme_sound(*which) {
            Some(file) => Command::new("pw-play")
                .args(["--target", &target.to_string()])
                .arg(&file)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|_| ()),
            None => tone(target, *which),
        };
        if let Err(err) = played {
            tracing::warn!(%err, "pw-play cannot play the test");
        }
        if let Some(rest) = EACH.checked_sub(start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    side(None);
}

/// The sound theme's file for `side`, in the data directories.
fn theme_sound(side: Side) -> Option<PathBuf> {
    let name = match side {
        Side::Left => "audio-channel-front-left",
        Side::Right => "audio-channel-front-right",
        Side::Middle => "audio-channel-front-center",
    };
    let system = std::env::var_os("XDG_DATA_DIRS")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::env::split_paths(&system).find_map(|dir| {
        ["oga", "ogg", "wav"].iter().find_map(|ext| {
            let file = Path::new(&dir).join(format!("sounds/freedesktop/stereo/{name}.{ext}"));
            file.is_file().then_some(file)
        })
    })
}

/// Our own tone: 0.4 s of 660 Hz, faded in and out, on `side`'s channel.
fn tone(target: u32, side: Side) -> std::io::Result<()> {
    let mut child = Command::new("pw-play")
        .args([
            "--target",
            &target.to_string(),
            "--raw",
            "--format",
            "s16",
            "--rate",
            &RATE.to_string(),
            "--channels",
            "2",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(&samples(side))?;
    }
    child.wait().map(|_| ())
}

/// The tone's samples, two channels interleaved.
fn samples(side: Side) -> Vec<u8> {
    let count = RATE as usize * 2 / 5;
    let fade = RATE as usize / 50;
    let (left, right) = match side {
        Side::Left => (1.0, 0.0),
        Side::Right => (0.0, 1.0),
        Side::Middle => (0.7, 0.7),
    };
    let mut out = Vec::with_capacity(count * 4);
    for i in 0..count {
        let t = i as f32 / RATE as f32;
        let envelope = (i.min(count - i) as f32 / fade as f32).min(1.0);
        let wave = (t * 660.0 * std::f32::consts::TAU).sin() * 0.4 * envelope;
        for gain in [left, right] {
            out.extend_from_slice(&((wave * gain * 32767.0) as i16).to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_is_on_its_side_only() {
        let left = samples(Side::Left);
        let quiet = |channel: usize| {
            left.as_chunks::<4>()
                .0
                .iter()
                .all(|f| i16::from_le_bytes([f[channel * 2], f[channel * 2 + 1]]) == 0)
        };
        assert!(quiet(1) && !quiet(0));
        assert_eq!(left.len(), RATE as usize * 2 / 5 * 4);
    }
}
