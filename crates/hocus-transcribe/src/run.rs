//! A meeting's text, from its folder: its record and its two tracks heard,
//! the user's echo of the call left out, its turns written in the
//! meeting's folder of documents; then its sound goes. On failure, all is
//! kept for "Réessayer".

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_meetings::{Heard as Who, RATE, Record, Said, Track, wav};
use jiff::{Timestamp, tz::TimeZone};

use crate::{engine::Engine, text};

/// What a run made: the file, who it heard, the sound's length (seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub file: PathBuf,
    pub who: Who,
    pub length: u64,
}

/// Says the progress, once a percent or a second has gone by.
struct Progress<'a> {
    say: &'a mut dyn FnMut(Said),
    started: Instant,
    last: Option<(f32, Instant)>,
}

impl Progress<'_> {
    fn at(&mut self, done: f32) {
        let now = Instant::now();
        if let Some((before, when)) = self.last
            && done - before < 0.01
            && now.duration_since(when) < Duration::from_secs(1)
        {
            return;
        }
        self.last = Some((done, now));
        // Once a twentieth is done, the rest at the pace so far.
        let left = (done >= 0.05).then(|| {
            let spent = now.duration_since(self.started).as_secs_f32();
            (spent * (1.0 - done) / done).round() as u64
        });
        (self.say)(Said::Working { done, left });
    }
}

/// Meeting folder `dir` heard by `engine`, its text written; `say` told
/// how far it is.
pub fn run(dir: &Path, engine: &mut dyn Engine, say: &mut dyn FnMut(Said)) -> anyhow::Result<Made> {
    let record = Record::read(dir)?;
    let track = |t: Track| {
        let path = dir.join(t.file());
        if path.exists() {
            wav::read(&path)
        } else {
            Ok(Vec::new())
        }
    };
    let (me, call) = (track(Track::Me)?, track(Track::Call)?);
    let length = (me.len().max(call.len()) as u64) / u64::from(RATE);
    let mut progress = Progress {
        say,
        started: Instant::now(),
        last: None,
    };
    progress.at(0.0);
    let heard = engine.hear(&me, &call, &mut |done| progress.at(done))?;
    let mut lines = text::without_echo(heard.me, &heard.call);
    lines.extend(heard.call);
    let turns = text::turns(lines);
    let zone = TimeZone::system();
    let markdown = text::markdown(&record, &turns, &zone)?;
    let start: Timestamp = record.start.parse()?;
    std::fs::create_dir_all(&record.folder)?;
    let file = unused(
        &record.folder,
        &text::file_name(&start.to_zoned(zone), &record.title),
    );
    let part = file.with_extension("md.part");
    std::fs::write(&part, markdown)?;
    std::fs::rename(&part, &file)?;
    forget(dir);
    Ok(Made {
        file,
        who: text::who(&turns),
        length,
    })
}

/// `name` in `dir`, or with " (2)", " (3)"… before its extension when a
/// file has it.
fn unused(dir: &Path, name: &str) -> PathBuf {
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

/// The meeting's sound and record go, then its folder.
fn forget(dir: &Path) {
    for file in [Track::Me.file(), Track::Call.file(), hocus_meetings::RECORD] {
        let path = dir.join(file);
        if let Err(err) = std::fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("hocus-transcribe: {}: {err}", path.display());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Fake;

    fn meeting(dir: &Path, folder: &Path) -> anyhow::Result<()> {
        Record {
            title: "Point produit".to_owned(),
            start: "2026-11-17T14:00:00Z".to_owned(),
            end: "2026-11-17T14:30:00Z".to_owned(),
            attendees: Vec::new(),
            began: "2026-11-17T14:00:00Z".to_owned(),
            lang: "en".to_owned(),
            folder: folder.to_owned(),
        }
        .write(dir)?;
        for track in Track::ALL {
            let mut writer = wav::Writer::create(&dir.join(track.file()))?;
            writer.write(&vec![0.0; RATE as usize * 3])?;
            writer.finish()?;
        }
        Ok(())
    }

    #[test]
    fn the_text_is_written_and_the_sound_goes_or_stays_on_failure() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let (dir, folder) = (root.path().join("m"), root.path().join("Meetings"));
        std::fs::create_dir_all(&dir)?;
        meeting(&dir, &folder)?;
        let mut said = Vec::new();
        let mut failing = Fake {
            fail: Some("no".to_owned()),
            ..Fake::default()
        };
        assert!(run(&dir, &mut failing, &mut |s| said.push(s)).is_err());
        assert!(dir.join(Track::Me.file()).exists());
        let mut fake = Fake {
            me: vec![(0.0, 1.0, "Hello.".to_owned())],
            call: vec![
                (70.0, 71.0, 1, "Hi.".to_owned()),
                (72.0, 73.0, 2, "Hey.".to_owned()),
            ],
            ..Fake::default()
        };
        let made = run(&dir, &mut fake, &mut |s| said.push(s))?;
        assert_eq!(made.file, folder.join("2026-11-17 14-00 Point produit.md"));
        assert_eq!(made.length, 3);
        assert_eq!((made.who.me, made.who.voices), (true, 2));
        let text = std::fs::read_to_string(&made.file)?;
        assert!(
            text.contains(
                "**14:00 · Me** — Hello.\n\n**14:01 · Voice 1** — Hi.\n\n**14:01 · Voice 2** — Hey."
            ),
            "{text}"
        );
        assert!(!dir.exists());
        assert!(
            said.iter()
                .any(|s| matches!(s, Said::Working { done, .. } if *done >= 1.0))
        );
        Ok(())
    }
}
