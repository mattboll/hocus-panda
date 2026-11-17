//! A meeting's transcription (docs/design/meetings.md): the shell asks
//! (the meeting card's Transcrire, `transcribe start`), the compositor
//! records the meeting's two tracks off the loop ([`capture`]) and tells
//! the bar how long it has (`transcribing`), until the shell says stop
//! (the card's stop, the meeting's end, the call away from the microphone
//! for 2 min). Then `hocus-transcribe` makes its text, at a low priority,
//! its progress in a notification ([`notes`]), replaced by "Transcription
//! prête" (Ouvrir, Dossier) or, on failure, "La transcription n'a pas pu se
//! faire" (Réessayer: the sound was kept).
//!
//! One recording at a time; the jobs that make texts may be several.
//! Nothing here waits: the tracks are written, the program read and the
//! notifications sent on threads of their own, which tell the loop
//! through a channel ([`FromWorkers`]).

pub mod capture;
mod notes;

use std::{
    collections::HashMap,
    io::BufRead,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use hocus_link::{ToShell, TranscribeAsk, TranscribeMeeting, TranscribingShown};
use hocus_meetings::{Heard, Record, Said};
use hocus_words::{Lang, compositor::transcribe as txt};
use smithay::reexports::calloop::channel::{self, Event, Sender};

use crate::{State, programs};
use capture::{Capture, Plan};
use notes::{Note, Notes};

/// What the workers tell the loop.
#[derive(Debug)]
pub enum FromWorkers {
    /// Recording `id`'s tracks are ended (`ok`), or could not be written.
    Recorded { id: u64, ok: bool },
    /// `hocus-transcribe` said this of job `id`.
    Said { id: u64, said: Said },
    /// It ended, successfully or not.
    Ended { id: u64, ok: bool },
    /// Job `id`'s notification's action `key` was chosen.
    Acted { id: u64, key: String },
    /// It was closed.
    Closed { id: u64 },
}

/// The recording under way.
#[derive(Debug)]
struct Live {
    id: u64,
    meeting: TranscribeMeeting,
    dir: PathBuf,
    capture: Capture,
    /// When it started, on the monotonic clock.
    since: Duration,
}

/// Where a job is.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Its tracks are being ended.
    Ending,
    /// `hocus-transcribe` runs.
    Making,
    /// Its text is in this file.
    Written(PathBuf),
    /// It could not be made; its sound is kept.
    Failed(String),
}

/// A meeting's text being made, or made.
#[derive(Debug)]
struct Job {
    dir: PathBuf,
    title: String,
    step: Step,
    /// The sound's length, seconds, once said.
    length: Option<u64>,
    notes: Option<Notes>,
    /// The progress last told, and when (the monotonic clock).
    told: Option<(i32, Duration)>,
    /// The user closed its progress: no more of it is said.
    quiet: bool,
}

/// What a test can know of the transcriptions.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscribeInfo {
    /// The recording under way: its id and its folder.
    pub recording: Option<(u64, PathBuf)>,
    /// Each job's folder and step, by id.
    pub jobs: Vec<(u64, PathBuf, Step)>,
}

/// The meetings' transcriptions, owned by the loop.
#[derive(Debug, Default)]
pub struct Transcription {
    live: Option<Live>,
    jobs: HashMap<u64, Job>,
    next: u64,
    back: Option<Sender<FromWorkers>>,
}

impl Transcription {
    pub fn recording(&self) -> bool {
        self.live.is_some()
    }

    pub fn info(&self) -> TranscribeInfo {
        let mut jobs: Vec<(u64, PathBuf, Step)> = self
            .jobs
            .iter()
            .map(|(id, job)| (*id, job.dir.clone(), job.step.clone()))
            .collect();
        jobs.sort_by_key(|(id, _, _)| *id);
        TranscribeInfo {
            recording: self.live.as_ref().map(|l| (l.id, l.dir.clone())),
            jobs,
        }
    }
}

/// "32 min", "1 h 05", "moins d'une minute": a length the user reads.
pub fn length(lang: Lang, seconds: u64) -> String {
    let minutes = (seconds + 30) / 60;
    match minutes {
        0 => txt::UNDER_A_MINUTE.get(lang).to_owned(),
        m if m < 60 => txt::minutes(lang, m),
        m => txt::hours(
            lang,
            txt::Hours {
                h: &(m / 60),
                m: &(m % 60),
            },
        ),
    }
}

/// "Moi et 2 voix", "Moi et les autres": who the text heard.
pub fn heard(lang: Lang, heard: Heard) -> String {
    match (heard.me, heard.voices, heard.others) {
        (true, 0, false) => txt::ME.get(lang).to_owned(),
        (true, 0, true) => txt::ME_AND_OTHERS.get(lang).to_owned(),
        (true, n, _) => txt::me_and_voices(lang, n),
        (false, 0, true) => txt::OTHERS.get(lang).to_owned(),
        (false, 0, false) => txt::NOBODY.get(lang).to_owned(),
        (false, n, _) => txt::voices(lang, n),
    }
}

impl State {
    /// The shell asks: a meeting's recording starts, or stops.
    pub fn transcribe_asked(&mut self, ask: TranscribeAsk) {
        match ask {
            TranscribeAsk::Start(meeting) => self.transcribe_start(meeting),
            TranscribeAsk::Stop => self.transcribe_stop(),
        }
    }

    fn transcribe_start(&mut self, meeting: TranscribeMeeting) {
        if self.transcribe.live.is_some() {
            tracing::info!("a meeting is being recorded: one at a time");
            return;
        }
        let Some(places) = self.config.meeting_places.clone() else {
            tracing::warn!("no place for a meeting's sound: it is not transcribed");
            return;
        };
        let Some(folder) = self.meetings_folder() else {
            tracing::warn!("no documents' folder: the meeting is not transcribed");
            return;
        };
        let Some(back) = self.transcribe_back() else {
            return;
        };
        self.transcribe.next += 1;
        let id = self.transcribe.next;
        let began = jiff::Timestamp::now();
        let name = format!("{}-{id}", began.strftime("%Y%m%d-%H%M%S"));
        let record = Record {
            title: meeting.title.clone(),
            start: meeting.start.clone(),
            end: meeting.end.clone(),
            attendees: meeting.attendees.clone(),
            began: began.to_string(),
            lang: self.config.lang.code().to_owned(),
            folder,
        };
        let plan = Plan {
            id,
            dir: places.meeting(&name),
            record,
            app: meeting.app.clone(),
            remote: self.config.pipewire_remote.clone(),
        };
        let dir = plan.dir.clone();
        let capture = match Capture::start(plan, back) {
            Ok(capture) => capture,
            Err(err) => {
                tracing::warn!(%err, "a meeting's recording does not start");
                return;
            }
        };
        tracing::info!(
            id,
            event = meeting.event,
            app = meeting.app,
            dir = %dir.display(),
            "a meeting's recording starts"
        );
        self.transcribe.live = Some(Live {
            id,
            meeting,
            dir,
            capture,
            since: self.now(),
        });
        self.tell_transcribing();
    }

    /// The recording stops: its tracks are ended off the loop, then its
    /// text is made.
    pub fn transcribe_stop(&mut self) {
        let Some(live) = self.transcribe.live.take() else {
            return;
        };
        tracing::info!(id = live.id, "a meeting's recording stops");
        live.capture.stop();
        self.transcribe.jobs.insert(
            live.id,
            Job {
                dir: live.dir,
                title: live.meeting.title,
                step: Step::Ending,
                length: None,
                notes: None,
                told: None,
                quiet: false,
            },
        );
        self.tell_transcribing();
    }

    /// The folder texts go in: `[meetings] folder`, else the documents'
    /// "Réunions" ("Meetings").
    fn meetings_folder(&self) -> Option<PathBuf> {
        self.config
            .chosen_folder(&self.config.meetings.folder)
            .or_else(|| {
                let documents = self.config.documents.as_ref()?;
                Some(documents.join(hocus_words::transcribe::MEETINGS.get(self.config.lang)))
            })
    }

    /// Where the workers say what happens, made once.
    fn transcribe_back(&mut self) -> Option<Sender<FromWorkers>> {
        if let Some(back) = &self.transcribe.back {
            return Some(back.clone());
        }
        let (sender, receiver) = channel::channel::<FromWorkers>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.transcribe_heard(said);
            }
        });
        match inserted {
            Ok(_) => {
                self.transcribe.back = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not listen to the meetings' workers");
                None
            }
        }
    }

    fn transcribe_heard(&mut self, from: FromWorkers) {
        match from {
            FromWorkers::Recorded { id, ok } => {
                tracing::info!(id, ok, "a meeting's tracks are ended");
                self.transcribe_make(id);
            }
            FromWorkers::Said { id, said } => self.transcribe_said(id, said),
            FromWorkers::Ended { id, ok } => {
                let Some(job) = self.transcribe.jobs.get_mut(&id) else {
                    return;
                };
                if job.step == Step::Making {
                    // Ended without saying how: it failed.
                    let why = if ok {
                        "hocus-transcribe said nothing".to_owned()
                    } else {
                        "hocus-transcribe ended".to_owned()
                    };
                    self.transcribe_said(id, Said::Failed { why });
                }
            }
            FromWorkers::Acted { id, key } => self.transcribe_acted(id, &key),
            FromWorkers::Closed { id } => {
                if let Some(job) = self.transcribe.jobs.get_mut(&id)
                    && job.step == Step::Making
                {
                    job.quiet = true;
                }
            }
        }
    }

    /// Job `id`'s text is made: `hocus-transcribe` started on its folder,
    /// its output read on a thread of its own.
    fn transcribe_make(&mut self, id: u64) {
        let Some(back) = self.transcribe_back() else {
            return;
        };
        let models = self.config.meeting_places.as_ref().map(|p| p.models());
        let Some(job) = self.transcribe.jobs.get_mut(&id) else {
            return;
        };
        job.step = Step::Making;
        job.told = None;
        job.quiet = false;
        let program = if self.config.transcriber.as_os_str().is_empty() {
            PathBuf::from("hocus-transcribe")
        } else {
            self.config.transcriber.clone()
        };
        let dir = job.dir.clone();
        tracing::info!(id, dir = %dir.display(), "a meeting's text is being made");
        programs::on_a_thread(format!("transcribe {id}"), move || {
            let mut command = Command::new(&program);
            command
                .arg("run")
                .arg(&dir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            if let Some(models) = &models {
                command.arg("--models").arg(models);
            }
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(err) => {
                    tracing::warn!(%err, program = %program.display(), "hocus-transcribe does not start");
                    let _ = back.send(FromWorkers::Ended { id, ok: false });
                    return;
                }
            };
            tracing::info!(id, pid = child.id(), "hocus-transcribe runs");
            if let Some(out) = child.stdout.take() {
                for line in std::io::BufReader::new(out).lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if let Some(said) = Said::read(&line) {
                        let _ = back.send(FromWorkers::Said { id, said });
                    }
                }
            }
            let ok = child.wait().is_ok_and(|s| s.success());
            let _ = back.send(FromWorkers::Ended { id, ok });
        });
        self.transcribe_note(id, None);
    }

    fn transcribe_said(&mut self, id: u64, said: Said) {
        let lang = self.config.lang;
        let Some(job) = self.transcribe.jobs.get_mut(&id) else {
            return;
        };
        match said {
            Said::Fetching { done, total } => {
                let value = (done.saturating_mul(100) / total.max(1)) as i32;
                let mb = |b: u64| txt::megabytes(lang, (b + 500_000) / 1_000_000);
                let body = txt::fetching(
                    lang,
                    txt::Fetching {
                        done: &mb(done),
                        total: &mb(total),
                    },
                );
                self.transcribe_progress(id, value, body);
            }
            Said::Working { done, left } => {
                let value = (done * 100.0).round() as i32;
                let sound = job.length.or_else(|| {
                    hocus_meetings::wav::seconds(&job.dir.join(hocus_meetings::Track::Me.file()))
                        .ok()
                        .map(|s| s.round() as u64)
                });
                job.length = sound;
                let of = length(lang, sound.unwrap_or(0));
                let body = match left {
                    Some(left) => txt::left(
                        lang,
                        txt::Left {
                            left: &length(lang, left),
                            length: &of,
                        },
                    ),
                    None => txt::of_sound(lang, of),
                };
                self.transcribe_progress(id, value, body);
            }
            Said::Written {
                file,
                heard: who,
                length: seconds,
            } => {
                tracing::info!(id, file = %file.display(), ?who, "a meeting's text is written");
                job.step = Step::Written(file);
                let note = Note {
                    summary: txt::READY.get(lang).to_owned(),
                    body: txt::ready_body(
                        lang,
                        txt::ReadyBody {
                            heard: &heard(lang, who),
                            length: &length(lang, seconds),
                            title: &job.title,
                        },
                    ),
                    value: None,
                    actions: vec![
                        ("default", String::new()),
                        ("open", txt::OPEN.get(lang).to_owned()),
                        ("folder", txt::FOLDER.get(lang).to_owned()),
                    ],
                    stays: false,
                };
                self.transcribe_note(id, Some(note));
            }
            Said::Failed { why } => {
                tracing::warn!(
                    id,
                    why,
                    "a meeting's text could not be made: its sound is kept"
                );
                job.step = Step::Failed(why);
                let note = Note {
                    summary: txt::FAILED.get(lang).to_owned(),
                    body: txt::failed_body(lang, &job.title),
                    value: None,
                    actions: vec![("retry", txt::RETRY.get(lang).to_owned())],
                    stays: false,
                };
                self.transcribe_note(id, Some(note));
            }
        }
    }

    /// The progress said, once it moved a percent and a second has gone by.
    fn transcribe_progress(&mut self, id: u64, value: i32, body: String) {
        let now = self.now();
        let lang = self.config.lang;
        let Some(job) = self.transcribe.jobs.get_mut(&id) else {
            return;
        };
        if job.quiet {
            return;
        }
        if let Some((before, when)) = job.told
            && (value == before || now.saturating_sub(when) < Duration::from_secs(1))
            && value < 100
        {
            return;
        }
        job.told = Some((value, now));
        let note = Note {
            summary: txt::working(lang, &job.title),
            body,
            value: Some(value),
            actions: Vec::new(),
            stays: true,
        };
        self.transcribe_note(id, Some(note));
    }

    /// Job `id`'s notification says `note` (none: its first progress,
    /// before the program said anything).
    fn transcribe_note(&mut self, id: u64, note: Option<Note>) {
        let lang = self.config.lang;
        let bus = self.shell.bus.clone();
        let back = self.transcribe_back();
        let Some(job) = self.transcribe.jobs.get_mut(&id) else {
            return;
        };
        let note = note.unwrap_or_else(|| Note {
            summary: txt::working(lang, &job.title),
            body: String::new(),
            value: Some(0),
            actions: Vec::new(),
            stays: true,
        });
        tracing::info!(
            id,
            summary = note.summary,
            body = note.body,
            value = ?note.value,
            "a meeting's notification says"
        );
        if job.notes.is_none()
            && let (Some(bus), Some(back)) = (bus, back)
        {
            job.notes = Some(notes::start(bus, id, back));
        }
        if let Some(notes) = &job.notes {
            notes.say(note);
        }
    }

    fn transcribe_acted(&mut self, id: u64, key: &str) {
        let Some(job) = self.transcribe.jobs.get(&id) else {
            return;
        };
        tracing::info!(id, key, "a meeting's notification is acted on");
        match (key, &job.step) {
            ("default" | "open", Step::Written(file)) => {
                let file = file.clone();
                self.transcribe_open(&file);
            }
            ("folder", Step::Written(file)) => {
                if let Some(folder) = file.parent() {
                    let folder = folder.to_owned();
                    self.transcribe_open(&folder);
                }
            }
            ("retry", Step::Failed(_)) => self.transcribe_make(id),
            _ => {}
        }
    }

    /// `path` opened by the default application, as the launcher opens an
    /// address (`xdg-open`).
    fn transcribe_open(&mut self, path: &Path) {
        let launch = hocus_link::Launch {
            id: "xdg-open".to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec!["xdg-open".to_owned(), path.to_string_lossy().into_owned()],
        };
        if !launch.fits() {
            tracing::warn!(path = %path.display(), "a path that cannot be opened");
            return;
        }
        let env = programs::Env::of(self).with_activation(self.activation_token());
        programs::launch(launch, None, env);
    }

    /// The bar's tile and the card's stop: the recording, or none.
    pub fn tell_transcribing(&mut self) {
        let now = self.now();
        let line = match &self.transcribe.live {
            Some(live) => ToShell::Transcribing(TranscribingShown {
                id: live.id,
                event: live.meeting.event.clone(),
                title: live.meeting.title.clone(),
                app: live.meeting.app.clone(),
                elapsed: u64::try_from(now.saturating_sub(live.since).as_millis())
                    .unwrap_or(u64::MAX),
            }),
            None => ToShell::TranscribingEnd,
        };
        self.tell_shell(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_who_was_heard_in_both_languages() {
        let (fr, en) = (Lang::French, Lang::English);
        assert_eq!(length(fr, 20), "moins d’une minute");
        assert_eq!(length(fr, 32 * 60), "32 min");
        assert_eq!(length(en, 3900), "1 h 05");
        let who = |me, voices, others| Heard { me, voices, others };
        assert_eq!(heard(fr, who(true, 2, false)), "Moi et 2 voix");
        assert_eq!(heard(en, who(true, 1, false)), "Me and 1 voice");
        assert_eq!(heard(fr, who(true, 0, true)), "Moi et les autres");
        assert_eq!(heard(fr, who(true, 0, false)), "Moi");
        assert_eq!(heard(en, who(false, 0, true)), "The others");
    }
}
