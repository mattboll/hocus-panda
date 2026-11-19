//! The Hocuspanda microphone's program (#619, lot 5, docs/design/voice.md):
//! `hocus-voice`, started by the compositor, publishes the "Hocuspanda"
//! audio source on PipeWire and reads the real microphone only while it is
//! read.
//!
//! - **Its life**, as `hocus-camera`'s: started from the loop's turn once
//!   its program is known (the session's, beside the compositor; a
//!   test's), on a thread of its own which reads what it says and hands it
//!   to the loop; started again when it ends, after 2 s, then twice as
//!   long each time up to a minute. It ends when its input does.
//! - **What it is told** ([`VoiceLook`]): the voice of `[voice] effect`, at
//!   once as it starts, then as that changes, looked at once a turn of the
//!   loop.
//! - **What it says** ([`FromVoice`]): on the graph, read or not, and the
//!   real microphone it reads or would read, which a meeting's
//!   transcription records (never the Hocuspanda one).

use std::{
    io::{BufRead, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use hocus_link::{FromVoice, VoiceLook, voice::Microphone};
use smithay::reexports::calloop::channel::{self, Event, Sender};

use crate::{State, programs};

/// The program is started again after this, doubled each time.
const AGAIN: Duration = Duration::from_secs(2);
const AGAIN_MOST: Duration = Duration::from_secs(60);

/// What the program's thread tells the loop.
#[derive(Debug)]
pub enum FromProgram {
    Started { pid: u32 },
    Said(FromVoice),
    Ended,
}

/// The Hocuspanda microphone, as the loop knows it.
#[derive(Debug, Default)]
pub struct HocusVoice {
    /// Its input's writer, while it runs.
    to: Option<mpsc::Sender<String>>,
    back: Option<Sender<FromProgram>>,
    /// The look it was told last.
    told: Option<VoiceLook>,
    /// When it may start (again), on the monotonic clock.
    start_at: Duration,
    again: Duration,
    /// Its source is on the graph.
    pub ready: bool,
    /// An application reads it.
    pub read: bool,
    /// The real microphone it reads, or would read.
    pub microphone: Option<Microphone>,
    /// The voice it makes, once made.
    pub made: Option<hocus_link::Voice>,
}

impl State {
    /// Once a turn of the loop: the program started when it should, told
    /// its voice when it changed.
    pub(crate) fn tick_voice(&mut self, now: Duration) {
        if self.voice.to.is_none()
            && self.config.voice_program.is_some()
            && now >= self.voice.start_at
        {
            self.voice_start(now);
        }
        if self.voice.to.is_some() {
            let look = VoiceLook {
                voice: self.config.voice.effect,
            };
            if self.voice.told.as_ref() != Some(&look) {
                if let Some(to) = &self.voice.to
                    && to.send(look.line()).is_err()
                {
                    self.voice.to = None;
                }
                self.voice.told = Some(look);
            }
        }
    }

    /// The real microphone's node name a meeting's transcription records:
    /// the one the Hocuspanda microphone reads, so never its own.
    pub(crate) fn real_microphone(&self) -> Option<String> {
        self.voice.microphone.as_ref().map(|m| m.name.clone())
    }

    fn voice_start(&mut self, now: Duration) {
        let Some(program) = self.config.voice_program.clone() else {
            return;
        };
        let Some(back) = self.voice_back() else {
            return;
        };
        self.voice.again = if self.voice.again.is_zero() {
            AGAIN
        } else {
            (self.voice.again * 2).min(AGAIN_MOST)
        };
        // Not again before its delay, whatever happens.
        self.voice.start_at = now + self.voice.again;
        let (to, lines) = mpsc::channel::<String>();
        self.voice.to = Some(to);
        self.voice.told = None;
        let remote = self.config.pipewire_remote.clone();
        programs::on_a_thread("hocus-voice".to_owned(), move || {
            run(&program, remote, lines, &back);
        });
    }

    fn voice_back(&mut self) -> Option<Sender<FromProgram>> {
        if let Some(back) = &self.voice.back {
            return Some(back.clone());
        }
        let (sender, receiver) = channel::channel::<FromProgram>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.voice_heard(said);
            }
        });
        match inserted {
            Ok(_) => {
                self.voice.back = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not listen to hocus-voice");
                None
            }
        }
    }

    fn voice_heard(&mut self, from: FromProgram) {
        match from {
            FromProgram::Started { pid } => tracing::info!(pid, "hocus-voice runs"),
            FromProgram::Said(said) => {
                tracing::debug!(?said, "hocus-voice says");
                match said {
                    FromVoice::Ready { node } => {
                        if !self.voice.ready {
                            tracing::info!(node, "the Hocuspanda microphone is on the graph");
                        }
                        self.voice.ready = true;
                        // Running well: a next ending starts it again soon.
                        self.voice.again = Duration::ZERO;
                    }
                    FromVoice::Read(read) => {
                        tracing::info!(read, "the Hocuspanda microphone");
                        self.voice.read = read;
                    }
                    FromVoice::Microphone(mic) => {
                        tracing::info!(?mic, "the Hocuspanda microphone's microphone");
                        self.voice.microphone = mic;
                    }
                    FromVoice::Voice(voice) => {
                        tracing::info!(?voice, "the Hocuspanda microphone's voice");
                        self.voice.made = Some(voice);
                    }
                }
            }
            FromProgram::Ended => {
                tracing::info!("hocus-voice ended");
                self.voice.to = None;
                self.voice.ready = false;
                self.voice.read = false;
                self.voice.made = None;
            }
        }
    }

    /// Tests: whether the Hocuspanda microphone is on the graph, read, and
    /// the real microphone it reads.
    pub(crate) fn voice_info(&self) -> (bool, bool, Option<String>) {
        (self.voice.ready, self.voice.read, self.real_microphone())
    }

    /// Tests: the voice it makes, once made.
    pub(crate) fn voice_made(&self) -> Option<hocus_link::Voice> {
        self.voice.made
    }
}

/// The program, on its thread: started, its output read and told, until
/// it ends; its input written from `lines` on a thread of its own.
fn run(
    program: &std::path::Path,
    remote: Option<PathBuf>,
    lines: mpsc::Receiver<String>,
    back: &Sender<FromProgram>,
) {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(remote) = remote {
        command.arg("--remote").arg(remote);
    }
    programs::default_signals(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(%err, program = %program.display(), "hocus-voice does not start");
            let _ = back.send(FromProgram::Ended);
            return;
        }
    };
    let _ = back.send(FromProgram::Started { pid: child.id() });
    let input = child.stdin.take();
    let output = child.stdout.take();
    std::thread::scope(|scope| {
        if let Some(mut input) = input {
            scope.spawn(move || {
                for line in lines {
                    if writeln!(input, "{line}")
                        .and_then(|()| input.flush())
                        .is_err()
                    {
                        break;
                    }
                }
                // Dropped: its input ends, and so does it.
            });
        }
        if let Some(output) = output {
            for line in std::io::BufReader::new(output).lines() {
                let Ok(line) = line else {
                    break;
                };
                if let Some(said) = FromVoice::read(&line) {
                    let _ = back.send(FromProgram::Said(said));
                }
            }
        }
        let _ = child.wait();
        let _ = back.send(FromProgram::Ended);
    });
}
