//! Throwaway: screenshots and a video of the levels on a headless output,
//! against the tests' fakes, for the pull request. Not committed.
//!
//! levels_shots OUT_DIR MODE   (MODE: states | video | cpu)
//! SHELL_BIN: the shell's program; RENDER_NODE: the GPU; SCALE: 1 or 2;
//! LANG_SHOT: the locale.

#[path = "../tests/levels/fakes.rs"]
mod fakes;

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};

const KEY_LEFTMETA: u32 = 125;
const KEY_B: u32 = 48;
const KEY_MUTE: u32 = 113;
const KEY_VOLUMEDOWN: u32 = 114;
const KEY_VOLUMEUP: u32 = 115;
const KEY_BRIGHTNESSDOWN: u32 = 224;
const KEY_BRIGHTNESSUP: u32 = 225;

fn chord(hp: &mut Headless, keys: &[u32]) {
    for k in keys {
        hp.key(*k, true);
    }
    for k in keys.iter().rev() {
        hp.key(*k, false);
    }
}

fn wait(hp: &mut Headless, t: Duration) -> anyhow::Result<()> {
    hp.dispatch_until(t, |_| false)?;
    Ok(())
}

/// The region `(x, y, w, h)` of output 0's pixels, in physical pixels.
fn crop(bytes: &[u8], width: i32, (x, y, w, h): (i32, i32, i32, i32)) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&bytes[start..start + (w * 4) as usize]);
    }
    out
}

fn png(bytes: &[u8], (w, h): (i32, i32), zoom: u32, file: &Path) -> anyhow::Result<()> {
    let mut ff = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgra",
            "-s",
        ])
        .arg(format!("{w}x{h}"))
        .args(["-i", "-", "-vf"])
        .arg(format!("scale=iw*{zoom}:ih*{zoom}:flags=neighbor"))
        .args(["-frames:v", "1"])
        .arg(file)
        .stdin(Stdio::piped())
        .spawn()?;
    ff.stdin
        .take()
        .map(|mut s| s.write_all(bytes))
        .transpose()?;
    ff.wait()?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let mode = args.next().unwrap_or_else(|| "states".into());
    let shell = PathBuf::from(std::env::var("SHELL_BIN")?);
    let scale: i32 = std::env::var("SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let lang = std::env::var("LANG_SHOT").unwrap_or_else(|_| "fr_FR.UTF-8".into());
    let cpu = mode == "cpu";
    let fakes = fakes::Fakes::start()?;
    let script = fakes.shell(&shell, &lang)?;
    let mut config = Config::default();
    config.panel.form = hocus_compositor::FormChoice::Full;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![format!("2560x1600@{scale}").parse()?],
            renderer: if cpu {
                RendererChoice::Pixman
            } else {
                RendererChoice::Gles
            },
            render_node: if cpu {
                None
            } else {
                Some(std::env::var("RENDER_NODE")?.into())
            },
        },
        config,
        Some(script),
    )?;
    eprintln!("renderer {}", hp.renderer());
    let ok = hp.dispatch_until(Duration::from_secs(20), |h| h.levels_commits() > 0)?;
    anyhow::ensure!(ok, "no levels");
    wait(&mut hp, Duration::from_millis(1500))?;
    // Logical: the bar's bottom right, 220 × 180.
    let (lw, lh) = (2560 / scale, 1600 / scale);
    let region = |full: bool| {
        let w = if full { 220 } else { 80 };
        ((lw - w) * scale, (lh - 180) * scale, w * scale, 180 * scale)
    };
    let shot = |hp: &mut Headless, name: &str, full: bool| -> anyhow::Result<()> {
        wait(hp, Duration::from_millis(400))?;
        let (w, _, bytes) = hp.screenshot(0)?;
        let r = region(full);
        let part = crop(&bytes, w, r);
        let suffix = if scale == 2 { "-2x" } else { "-1x" };
        png(
            &part,
            (r.2, r.3),
            1,
            &out.join(format!("{name}{suffix}.png")),
        )?;
        if scale == 1 {
            png(
                &part,
                (r.2, r.3),
                2,
                &out.join(format!("{name}-1x-zoom2.png")),
            )?;
        }
        Ok(())
    };
    match mode.as_str() {
        "states" | "cpu" => {
            let prefix = if cpu { "cpu-" } else { "" };
            for full in [true, false] {
                let form = if full { "full" } else { "mini" };
                if !full {
                    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
                    wait(&mut hp, Duration::from_millis(600))?;
                }
                fakes.set_power(|p| *p = fakes::Power::default())?;
                shot(&mut hp, &format!("{prefix}{form}-discharging"), full)?;
                if cpu {
                    continue;
                }
                fakes.set_power(|p| {
                    p.percentage = 45.0;
                    p.state = 1;
                    p.to_full = 70 * 60;
                })?;
                shot(&mut hp, &format!("{form}-charging"), full)?;
                fakes.set_power(|p| {
                    p.percentage = 100.0;
                    p.state = 4;
                })?;
                shot(&mut hp, &format!("{form}-full"), full)?;
                fakes.set_power(|p| {
                    p.percentage = 14.0;
                    p.state = 2;
                    p.to_empty = 35 * 60;
                })?;
                shot(&mut hp, &format!("{form}-low"), full)?;
                fakes.set_power(|p| {
                    p.percentage = 6.0;
                    p.to_empty = 9 * 60;
                })?;
                shot(&mut hp, &format!("{form}-critical"), full)?;
                fakes.set_power(|p| *p = fakes::Power::default())?;
                chord(&mut hp, &[KEY_MUTE]);
                shot(&mut hp, &format!("{form}-muted"), full)?;
                chord(&mut hp, &[KEY_MUTE]);
                fakes.set_power(|p| {
                    p.present = false;
                    p.profile = "power-saver".into();
                })?;
                shot(&mut hp, &format!("{form}-no-battery"), full)?;
                fakes.set_power(|p| *p = fakes::Power::default())?;
                fakes.remove_backlight()?;
                shot(&mut hp, &format!("{form}-no-backlight"), full)?;
                fakes.backlight(700, 1000)?;
                wait(&mut hp, Duration::from_millis(300))?;
            }
        }
        "video" => video(&mut hp, &out, scale)?,
        _ => anyhow::bail!("mode?"),
    }
    let (levels, bars, frames) = (hp.levels_commits(), hp.bar_commits(), hp.frames());
    wait(&mut hp, Duration::from_secs(2))?;
    eprintln!(
        "at rest over 2 s: {} levels commits, {} bar commits, {} frames",
        hp.levels_commits() - levels,
        hp.bar_commits() - bars,
        hp.frames() - frames
    );
    Ok(())
}

/// Volume keys pressed, held, mute, and brightness: the full form's bottom
/// at 60 frames a second of real time, ×3 with nearest pixels.
fn video(hp: &mut Headless, out: &Path, scale: i32) -> anyhow::Result<()> {
    let (lw, lh) = (2560 / scale, 1600 / scale);
    let r = (
        (lw - 220) * scale,
        (lh - 180) * scale,
        220 * scale,
        180 * scale,
    );
    let (w, _, first) = hp.screenshot(0)?;
    let start = Instant::now();
    let mut shots: Vec<(f64, Vec<u8>)> = vec![(0.0, crop(&first, w, r))];
    let mut capture = |hp: &mut Headless, until: f64| -> anyhow::Result<()> {
        let mut frames = hp.frames();
        while start.elapsed().as_secs_f64() < until {
            hp.dispatch(Duration::from_millis(1))?;
            if hp.frames() != frames {
                frames = hp.frames();
                let at = start.elapsed().as_secs_f64();
                let (_, _, bytes) = hp.screenshot(0)?;
                shots.push((at, crop(&bytes, w, r)));
            }
        }
        Ok(())
    };
    let mut at = 0.6;
    capture(hp, at)?;
    // Three presses up, three down.
    for key in [
        KEY_VOLUMEUP,
        KEY_VOLUMEUP,
        KEY_VOLUMEUP,
        KEY_VOLUMEDOWN,
        KEY_VOLUMEDOWN,
        KEY_VOLUMEDOWN,
    ] {
        hp.key(key, true);
        hp.key(key, false);
        at += 0.4;
        capture(hp, at)?;
    }
    // Held down 1.2 s, then up 1.2 s: the key repeats.
    hp.key(KEY_VOLUMEDOWN, true);
    at += 1.2;
    capture(hp, at)?;
    hp.key(KEY_VOLUMEDOWN, false);
    at += 0.4;
    capture(hp, at)?;
    hp.key(KEY_VOLUMEUP, true);
    at += 1.2;
    capture(hp, at)?;
    hp.key(KEY_VOLUMEUP, false);
    at += 0.4;
    capture(hp, at)?;
    // Mute and back.
    for _ in 0..2 {
        hp.key(KEY_MUTE, true);
        hp.key(KEY_MUTE, false);
        at += 0.8;
        capture(hp, at)?;
    }
    // Brightness down twice, up twice.
    for key in [
        KEY_BRIGHTNESSDOWN,
        KEY_BRIGHTNESSDOWN,
        KEY_BRIGHTNESSUP,
        KEY_BRIGHTNESSUP,
    ] {
        hp.key(key, true);
        hp.key(key, false);
        at += 0.4;
        capture(hp, at)?;
    }
    at += 0.6;
    capture(hp, at)?;
    eprintln!("{} frames", shots.len());
    let zoom = if scale == 1 { 3 } else { 2 };
    let mut ff = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgra",
            "-s",
        ])
        .arg(format!("{}x{}", r.2, r.3))
        .args(["-r", "60", "-i", "-", "-vf"])
        .arg(format!("scale=iw*{zoom}:ih*{zoom}:flags=neighbor"))
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16"])
        .arg(out.join("volume-keys.mp4"))
        .stdin(Stdio::piped())
        .spawn()?;
    let mut stdin = ff.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
    let total = (at * 60.0) as usize;
    let mut k = 0;
    for i in 0..total {
        let t = i as f64 / 60.0;
        while k + 1 < shots.len() && shots[k + 1].0 <= t {
            k += 1;
        }
        stdin.write_all(&shots[k].1)?;
    }
    drop(stdin);
    ff.wait()?;
    Ok(())
}
