//! Where a recording goes and what it is named, and how ffmpeg is asked to
//! encode it: pure functions, the recorder's thread runs them.

use hocus_words::compositor::record_save as txt;
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use hocus_link::{RECORD_FPS, RecordFormat};

/// The videos' folder: `$XDG_VIDEOS_DIR`, else what
/// `$XDG_CONFIG_HOME/user-dirs.dirs` says, else `~/Videos`.
pub fn videos_dir() -> Option<PathBuf> {
    crate::capture::save::xdg_dir("XDG_VIDEOS_DIR", "Videos")
}

/// The folder recordings go in, in the videos' folder.
pub fn folder_name(lang: hocus_words::Lang) -> &'static str {
    txt::SCREEN_RECORDINGS.get(lang)
}

/// The folder as the thumbnail and the history say it: the videos'
/// folder's own name, then the recordings'.
pub fn shown_folder(videos: &Path, lang: hocus_words::Lang) -> String {
    let base = videos
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if base.is_empty() {
        folder_name(lang).to_owned()
    } else {
        format!("{base}/{}", folder_name(lang))
    }
}

/// "Enregistrement du 2026-11-03 à 14-32-05.mp4", "Screen recording from
/// 2026-11-03 14-32-05.mp4", at `when` in the local time zone.
pub fn file_name(when: SystemTime, lang: hocus_words::Lang, format: RecordFormat) -> String {
    let zoned = jiff::Timestamp::try_from(when)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
        .unwrap_or_else(|_| jiff::Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::TimeZone::UTC));
    let day = zoned.strftime("%Y-%m-%d");
    let time = zoned.strftime("%H-%M-%S");
    let ext = format.name();
    txt::screen_recording_from(
        lang,
        txt::ScreenRecordingFrom {
            day: &day,
            ext: &ext,
            time: &time,
        },
    )
}

/// `name` in `dir`, or with " (2)", " (3)"… before its extension when a
/// file has it.
pub fn unused(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..1000)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// `(w, h)` made even, as H.264's 4:2:0 wants them, and at least 2.
pub fn even((w, h): (u32, u32)) -> (u32, u32) {
    ((w & !1).max(2), (h & !1).max(2))
}

/// The video encoder ffmpeg is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Video {
    /// The GPU's, through VA-API on this render node.
    Vaapi(PathBuf),
    /// On the CPU: libx264, libopenh264 or libvpx-vp9, by name.
    Software(&'static str),
}

impl Video {
    pub fn name(&self) -> String {
        match self {
            Self::Vaapi(node) => format!("vaapi ({})", node.display()),
            Self::Software(name) => (*name).to_owned(),
        }
    }
}

/// The software encoders that make `format`'s video, best first.
pub fn software(format: RecordFormat) -> &'static [&'static str] {
    match format {
        RecordFormat::Mp4 => &["libx264", "libopenh264"],
        RecordFormat::WebM => &["libvpx-vp9"],
    }
}

/// The encoders `ffmpeg -encoders` lists.
pub fn listed_encoders(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let flags = words.next()?;
            // " V....D libx264  …": a flags' word of six, then the name.
            (flags.len() == 6 && flags.chars().all(|c| c.is_ascii_uppercase() || c == '.'))
                .then(|| words.next())
                .flatten()
                .filter(|name| name.starts_with(|c: char| c.is_ascii_alphanumeric()))
                .map(str::to_owned)
        })
        .collect()
}

/// What ffmpeg is told to read and write.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The frames' size, even.
    pub size: (u32, u32),
    pub video: Video,
    pub format: RecordFormat,
    /// The sound's pipe (raw 32-bit float, 48 kHz, stereo); none: no
    /// sound track.
    pub audio: Option<PathBuf>,
    /// How late the sound is on the frames: the frames are put as late.
    pub audio_late: std::time::Duration,
    pub file: PathBuf,
}

/// The samples a second, and the channels, of the sound given to ffmpeg.
pub const RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

/// ffmpeg's arguments for `plan`: the frames on its standard input (B, G,
/// R, x rows, [`RECORD_FPS`] a second), the sound from its pipe, the file
/// written as `plan.format` says.
pub fn ffmpeg_args(plan: &Plan) -> Vec<String> {
    let (w, h) = plan.size;
    let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .map(str::to_owned)
        .to_vec();
    if let Video::Vaapi(node) = &plan.video {
        args.extend([
            "-init_hw_device".to_owned(),
            format!("vaapi=va:{}", node.display()),
            "-filter_hw_device".to_owned(),
            "va".to_owned(),
        ]);
    }
    if plan.audio.is_some() && !plan.audio_late.is_zero() {
        args.extend([
            "-itsoffset".to_owned(),
            format!("{:.3}", plan.audio_late.as_secs_f64()),
        ]);
    }
    args.extend([
        "-f".to_owned(),
        "rawvideo".to_owned(),
        "-pix_fmt".to_owned(),
        "bgr0".to_owned(),
        "-video_size".to_owned(),
        format!("{w}x{h}"),
        "-framerate".to_owned(),
        RECORD_FPS.to_string(),
        "-i".to_owned(),
        "pipe:0".to_owned(),
    ]);
    if let Some(audio) = &plan.audio {
        args.extend([
            "-f".to_owned(),
            "f32le".to_owned(),
            "-ar".to_owned(),
            RATE.to_string(),
            "-ac".to_owned(),
            CHANNELS.to_string(),
            "-i".to_owned(),
            audio.display().to_string(),
        ]);
    }
    args.extend(["-map".to_owned(), "0:v".to_owned()]);
    if plan.audio.is_some() {
        args.extend(["-map".to_owned(), "1:a".to_owned()]);
    }
    let gop = (2 * RECORD_FPS).to_string();
    match (&plan.video, plan.format) {
        (Video::Vaapi(_), RecordFormat::Mp4) => args.extend(
            [
                "-vf",
                "format=nv12,hwupload",
                "-c:v",
                "h264_vaapi",
                "-qp",
                "24",
                "-g",
                &gop,
            ]
            .map(str::to_owned),
        ),
        (Video::Vaapi(_), RecordFormat::WebM) => args.extend(
            [
                "-vf",
                "format=nv12,hwupload",
                "-c:v",
                "vp9_vaapi",
                "-global_quality",
                "60",
                "-g",
                &gop,
            ]
            .map(str::to_owned),
        ),
        (Video::Software("libx264"), _) => args.extend(
            [
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-tune",
                "zerolatency",
                "-crf",
                "23",
                "-pix_fmt",
                "yuv420p",
                "-g",
                &gop,
            ]
            .map(str::to_owned),
        ),
        (Video::Software("libvpx-vp9"), _) => args.extend(
            [
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "8",
                "-row-mt",
                "1",
                "-b:v",
                "0",
                "-crf",
                "34",
                "-pix_fmt",
                "yuv420p",
                "-g",
                &gop,
            ]
            .map(str::to_owned),
        ),
        (Video::Software(other), _) => args.extend(
            [
                "-c:v", other, "-b:v", "8M", "-pix_fmt", "yuv420p", "-g", &gop,
            ]
            .map(str::to_owned),
        ),
    }
    args.extend(["-r".to_owned(), RECORD_FPS.to_string()]);
    if plan.audio.is_some() {
        let (codec, rate) = match plan.format {
            RecordFormat::Mp4 => ("aac", "160k"),
            RecordFormat::WebM => ("libopus", "128k"),
        };
        args.extend(["-c:a", codec, "-b:a", rate].map(str::to_owned));
    }
    let muxer = match plan.format {
        RecordFormat::Mp4 => "mp4",
        RecordFormat::WebM => "webm",
    };
    args.extend(["-f".to_owned(), muxer.to_owned()]);
    args.push(plan.file.display().to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_name_says_when_in_the_users_language() {
        let fr = file_name(
            SystemTime::UNIX_EPOCH,
            hocus_words::Lang::French,
            RecordFormat::Mp4,
        );
        assert!(fr.starts_with("Enregistrement du ") && fr.contains(" à ") && fr.ends_with(".mp4"));
        let en = file_name(
            SystemTime::UNIX_EPOCH,
            hocus_words::Lang::English,
            RecordFormat::WebM,
        );
        assert!(en.starts_with("Screen recording from ") && en.ends_with(".webm"));
        let stamp = en
            .trim_start_matches("Screen recording from ")
            .trim_end_matches(".webm");
        assert_eq!(stamp.len(), 19);
        assert!(!stamp.contains(':'));
        assert_eq!(
            folder_name(hocus_words::Lang::French),
            "Enregistrements d'écran"
        );
        assert_eq!(
            shown_folder(Path::new("/home/alex/Vidéos"), hocus_words::Lang::French),
            "Vidéos/Enregistrements d'écran"
        );
    }

    #[test]
    fn a_second_file_in_the_same_second_goes_beside() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let name = "Screen recording from 2026-11-03 14-32-05.mp4";
        assert_eq!(unused(dir.path(), name), dir.path().join(name));
        std::fs::write(dir.path().join(name), b"")?;
        assert_eq!(
            unused(dir.path(), name),
            dir.path()
                .join("Screen recording from 2026-11-03 14-32-05 (2).mp4")
        );
        Ok(())
    }

    #[test]
    fn sizes_are_made_even() {
        assert_eq!(even((901, 761)), (900, 760));
        assert_eq!(even((2560, 1600)), (2560, 1600));
        assert_eq!(even((1, 0)), (2, 2));
    }

    #[test]
    fn the_encoders_listed_are_read() {
        let out = "Encoders:\n V..... = Video\n ------\n V....D libx264              libx264 H.264\n V....D h264_vaapi           H.264/AVC (VAAPI)\n A....D aac                  AAC\n";
        assert_eq!(
            listed_encoders(out),
            ["libx264", "h264_vaapi", "aac"].map(str::to_owned)
        );
    }

    #[test]
    fn ffmpeg_reads_the_frames_and_the_sound_and_writes_the_format() {
        let plan = Plan {
            size: (900, 760),
            video: Video::Software("libx264"),
            format: RecordFormat::Mp4,
            audio: Some(PathBuf::from("/run/x/sound")),
            audio_late: std::time::Duration::from_millis(100),
            file: PathBuf::from("/v/a.mp4"),
        };
        let args = ffmpeg_args(&plan).join(" ");
        assert!(args.contains("-itsoffset 0.100 -f rawvideo -pix_fmt bgr0 -video_size 900x760 -framerate 30 -i pipe:0"), "{args}");
        assert!(
            args.contains("-f f32le -ar 48000 -ac 2 -i /run/x/sound"),
            "{args}"
        );
        assert!(
            args.contains("-c:v libx264") && args.contains("-c:a aac"),
            "{args}"
        );
        assert!(
            args.ends_with("-r 30 -c:a aac -b:a 160k -f mp4 /v/a.mp4"),
            "{args}"
        );
        let silent = Plan {
            audio: None,
            video: Video::Vaapi(PathBuf::from("/dev/dri/renderD128")),
            ..plan
        };
        let args = ffmpeg_args(&silent).join(" ");
        assert!(args.contains("vaapi=va:/dev/dri/renderD128"), "{args}");
        assert!(
            args.contains("format=nv12,hwupload -c:v h264_vaapi"),
            "{args}"
        );
        assert!(
            !args.contains("-c:a") && !args.contains("itsoffset"),
            "{args}"
        );
    }
}
