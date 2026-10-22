//! What the trim editor asks of a recording (#132), through the encoder
//! (ffmpeg and ffprobe) as processes of their own: its length and size,
//! thumbnails, a frame at a time, its frames as it plays, and the export,
//! cut, in a format and under a size. Everything here blocks: the editor
//! calls it on worker threads.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

/// A recording's picture, as the encoder reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub length: Duration,
    pub sound: bool,
}

/// Frames as RGBA, all of one size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frames {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<Vec<u8>>,
}

/// Where the export goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp4,
    WebM,
    Gif,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            Self::Gif => "gif",
        }
    }
}

/// What to export.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub from: Duration,
    pub to: Duration,
    pub format: Format,
    /// The most the file may weigh, bytes.
    pub limit: Option<u64>,
}

fn seconds(d: Duration) -> String {
    format!("{:.3}", d.as_secs_f64())
}

fn run(command: &mut Command) -> Result<Vec<u8>, String> {
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        Err(said
            .lines()
            .last()
            .map_or_else(|| format!("ended with {}", out.status), str::to_owned))
    }
}

/// The recording's size, length, and whether it has sound.
pub fn info(probe: &str, file: &Path) -> Result<Info, String> {
    let out = run(Command::new(probe)
        .args(["-v", "error", "-show_entries"])
        .args([
            "stream=codec_type,width,height:format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(file))?;
    let text = String::from_utf8_lossy(&out);
    let (mut width, mut height, mut length, mut sound) = (0, 0, None, false);
    for line in text.lines() {
        let fields: Vec<&str> = line.split(',').collect();
        match fields.as_slice() {
            ["video", w, h, ..] => {
                width = w.parse().unwrap_or(0);
                height = h.parse().unwrap_or(0);
            }
            ["audio", ..] => sound = true,
            [d] => length = d.parse::<f64>().ok(),
            _ => {}
        }
    }
    match (width, height, length) {
        (w, h, Some(l)) if w > 0 && h > 0 && l > 0.0 => Ok(Info {
            width: w,
            height: h,
            length: Duration::from_secs_f64(l),
            sound,
        }),
        _ => Err(format!("{}: not a video", file.display())),
    }
}

/// `count` frames spread over the recording, `height` pixels high.
pub fn thumbnails(
    encoder: &str,
    file: &Path,
    info: &Info,
    count: u32,
    height: u32,
) -> Result<Frames, String> {
    let width = (info.width * height / info.height.max(1)).max(2) / 2 * 2;
    let rate = f64::from(count) / info.length.as_secs_f64().max(0.1);
    let out = run(Command::new(encoder)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args([
            "-vf",
            &format!("fps={rate:.4},scale={width}:{height}"),
            "-frames:v",
            &count.to_string(),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ]))?;
    let size = (width * height * 4) as usize;
    Ok(Frames {
        width,
        height,
        rgba: out.chunks_exact(size).map(<[u8]>::to_vec).collect(),
    })
}

/// The frame at `at`, `width` pixels wide.
pub fn frame_at(
    encoder: &str,
    file: &Path,
    info: &Info,
    at: Duration,
    width: u32,
) -> Result<Frames, String> {
    let height = (info.height * width / info.width.max(1)).max(2) / 2 * 2;
    let out = run(Command::new(encoder)
        .args(["-v", "error", "-ss", &seconds(at), "-i"])
        .arg(file)
        .args(["-vf", &format!("scale={width}:{height}")])
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"]))?;
    let size = (width * height * 4) as usize;
    if out.len() < size {
        return Err("no frame there".into());
    }
    Ok(Frames {
        width,
        height,
        rgba: vec![out[..size].to_vec()],
    })
}

/// The recording playing from `from` to `to`, at its pace, `width` pixels
/// wide: each frame goes to `frame` until it says to stop.
pub fn play(
    encoder: &str,
    file: &Path,
    info: &Info,
    (from, to): (Duration, Duration),
    width: u32,
    mut frame: impl FnMut(Vec<u8>, u32, u32) -> bool,
) -> Result<(), String> {
    let height = (info.height * width / info.width.max(1)).max(2) / 2 * 2;
    let mut child: Child = Command::new(encoder)
        .args([
            "-v",
            "error",
            "-re",
            "-ss",
            &seconds(from),
            "-to",
            &seconds(to),
            "-i",
        ])
        .arg(file)
        .args(["-vf", &format!("scale={width}:{height},fps=20")])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut out = child.stdout.take().ok_or("no picture")?;
    let mut buffer = vec![0; (width * height * 4) as usize];
    while out.read_exact(&mut buffer).is_ok() {
        if !frame(buffer.clone(), width, height) {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// A free name beside `file`: ` (trimmed)` and the format's extension.
pub fn beside(file: &Path, format: Format) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Recording".into());
    let ext = format.extension();
    (1..1000)
        .map(|n| match n {
            1 => dir.join(format!("{stem} (trimmed).{ext}")),
            n => dir.join(format!("{stem} (trimmed {n}).{ext}")),
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} (trimmed).{ext}")))
}

/// Writes the cut into `out`, in its format, under its limit when there is
/// one: smaller each try until it fits. The file written.
pub fn export(
    encoder: &str,
    file: &Path,
    info: &Info,
    what: &Export,
    out: &Path,
) -> Result<PathBuf, String> {
    let length = what.to.saturating_sub(what.from).as_secs_f64().max(0.1);
    let tries: Vec<Vec<String>> = match (what.format, what.limit) {
        (Format::Gif, limit) => {
            let widths: &[u32] = if limit.is_some() {
                &[800, 640, 480, 360, 240, 160]
            } else {
                &[800]
            };
            widths
                .iter()
                .map(|w| {
                    let w = (*w).min(info.width);
                    let fps = if w >= 640 { 15 } else { 12 };
                    vec![
                        "-vf".into(),
                        format!(
                            "fps={fps},scale={w}:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer"
                        ),
                        "-loop".into(),
                        "0".into(),
                    ]
                })
                .collect()
        }
        (format, None) => vec![codec(format, None, info.sound)],
        (format, Some(limit)) => {
            // Bits a second for the picture, sound's share taken out, a
            // little kept for the container; lower each try.
            let sound = if info.sound { 96_000.0 } else { 0.0 };
            let budget = (limit as f64 * 8.0 * 0.92 / length - sound).max(50_000.0);
            [1.0, 0.8, 0.6, 0.4]
                .iter()
                .map(|k| codec(format, Some((budget * k) as u64), info.sound))
                .collect()
        }
    };
    let mut last = String::new();
    for args in tries {
        let done = run(Command::new(encoder)
            .args([
                "-v",
                "error",
                "-y",
                "-ss",
                &seconds(what.from),
                "-to",
                &seconds(what.to),
                "-i",
            ])
            .arg(file)
            .args(&args)
            .arg(out));
        match done {
            Ok(_) => {
                let size = std::fs::metadata(out).map_err(|e| e.to_string())?.len();
                if what.limit.is_none_or(|l| size <= l) {
                    return Ok(out.to_path_buf());
                }
                last = format!("{size} bytes, over the limit");
            }
            Err(err) => last = err,
        }
    }
    let _ = std::fs::remove_file(out);
    Err(if last.is_empty() {
        "it would not fit".into()
    } else {
        last
    })
}

/// The codec's arguments: quality, or a rate in bits a second.
fn codec(format: Format, rate: Option<u64>, sound: bool) -> Vec<String> {
    let mut args: Vec<String> = match (format, rate) {
        (Format::WebM, None) => ["-c:v", "libvpx-vp9", "-crf", "33", "-b:v", "0"]
            .map(String::from)
            .to_vec(),
        (Format::WebM, Some(r)) => vec![
            "-c:v".into(),
            "libvpx-vp9".into(),
            "-b:v".into(),
            r.to_string(),
            "-maxrate".into(),
            r.to_string(),
            "-bufsize".into(),
            (r * 2).to_string(),
        ],
        (_, None) => ["-c:v", "libx264", "-preset", "veryfast", "-crf", "23"]
            .map(String::from)
            .to_vec(),
        (_, Some(r)) => vec![
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-b:v".into(),
            r.to_string(),
            "-maxrate".into(),
            r.to_string(),
            "-bufsize".into(),
            (r * 2).to_string(),
        ],
    };
    args.extend(["-pix_fmt".into(), "yuv420p".into()]);
    match (format, sound) {
        (_, false) => args.push("-an".into()),
        (Format::WebM, true) => args.extend(["-c:a", "libopus", "-b:a", "96k"].map(String::from)),
        (_, true) => args.extend(["-c:a", "aac", "-b:a", "96k"].map(String::from)),
    }
    if format == Format::Mp4 {
        args.extend(["-movflags", "+faststart"].map(String::from));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have(program: &str) -> bool {
        Command::new(program).arg("-version").output().is_ok()
    }

    /// Ten seconds of a busy test picture with a tone.
    fn sample(dir: &Path) -> PathBuf {
        let path = dir.join("Recording.mp4");
        run(Command::new("ffmpeg")
            .args(["-v", "error", "-y"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30:duration=10",
            ])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=10"])
            .args([
                "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-c:a", "aac",
            ])
            .arg(&path))
        .expect("a sample");
        path
    }

    #[test]
    fn a_cut_lasts_what_was_kept_and_fits_its_limit() {
        if !have("ffmpeg") || !have("ffprobe") {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        let dir = tempfile::tempdir().expect("a directory");
        let file = sample(dir.path());
        let i = info("ffprobe", &file).expect("its info");
        assert_eq!((i.width, i.height, i.sound), (640, 360, true));
        assert!((i.length.as_secs_f64() - 10.0).abs() < 0.1);

        let thumbs = thumbnails("ffmpeg", &file, &i, 8, 40).expect("thumbnails");
        assert_eq!((thumbs.rgba.len(), thumbs.height), (8, 40));
        let frame = frame_at("ffmpeg", &file, &i, Duration::from_secs(3), 320).expect("a frame");
        assert_eq!((frame.width, frame.height), (320, 180));

        let s = |n| Duration::from_secs(n);
        let cut = Export {
            from: s(2),
            to: s(6),
            format: Format::Mp4,
            limit: None,
        };
        let out = export("ffmpeg", &file, &i, &cut, &beside(&file, Format::Mp4)).expect("an mp4");
        assert!(out.ends_with("Recording (trimmed).mp4"));
        let kept = info("ffprobe", &out).expect("read back");
        assert!((kept.length.as_secs_f64() - 4.0).abs() < 0.15, "{kept:?}");

        let limit = 300_000;
        let gif = Export {
            format: Format::Gif,
            limit: Some(limit),
            ..cut
        };
        let out = export("ffmpeg", &file, &i, &gif, &beside(&file, Format::Gif)).expect("a gif");
        let size = std::fs::metadata(&out).expect("a file").len();
        assert!(size <= limit, "{size} bytes");

        let webm = Export {
            from: s(1),
            to: s(2),
            format: Format::WebM,
            limit: Some(200_000),
        };
        let out = export("ffmpeg", &file, &i, &webm, &beside(&file, Format::WebM)).expect("a webm");
        let kept = info("ffprobe", &out).expect("read back");
        assert!((kept.length.as_secs_f64() - 1.0).abs() < 0.15, "{kept:?}");
        assert!(kept.sound, "the sound is kept");
        assert!(std::fs::metadata(&out).expect("a file").len() <= 200_000);
    }
}
