//! The secretary's speak gate (docs/design/secretary.md §6.3), the
//! shell's: she asks to speak ([`asked`]); a thread of its own (never
//! iced's loop) holds the gate ([`hocus_secretary_link::gate`]), reads
//! the screen's state from what the shell already has when it looks
//! (a meeting of the featured calendars taking time now, do not disturb,
//! a share or a recording, the camera or the microphone, the lock or the
//! idle curtain, a full-screen window), and answers her: shown, held or
//! dropped. Shown, the compositor brings her peek out ([`peek`]) as an
//! agent's panda.
//!
//! The thread sleeps until something changes ([`poke`]: the in-use sign,
//! do not disturb, the lock, the curtain, the full-screen screens, the
//! day) or the gate's next moment (a meeting's end, the spacing, a
//! deadline, the next meeting's start for the moment): nothing is polled.
//! It also says the moment (§7.4) to her and to the drawer, which hides
//! perso in public ones ([`moment`]).
//!
//! In classic mode none of this runs: the thread starts with her first
//! request and stops when she goes ([`off`]).
//!
//! Typing (a key within 20 s) is in the gate but not told it yet: the
//! compositor does not say when keys are typed.

pub mod peek;

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::Duration,
};

use hocus_link::ToCompositor;
use hocus_secretary_link::{
    Moment, Speak, Spoken,
    gate::{Gate, Restraint, Screen, Verdict},
};
use jiff::{SignedDuration, Timestamp};

/// What the gate's thread is told.
#[derive(Debug)]
enum Input {
    Asked(Speak),
    Restraint(Restraint),
    /// Something the screen's state is read from changed.
    Poke,
    /// Her peek of `id` went back in.
    Back(u64),
    Stop,
}

static GATE: Mutex<Option<Sender<Input>>> = Mutex::new(None);

/// The moment as last seen, for the drawer: [`Moment`] as a number.
static MOMENT: AtomicU8 = AtomicU8::new(0);

fn moment_of(n: u8) -> Moment {
    match n {
        1 => Moment::Meeting,
        2 => Moment::Sharing,
        3 => Moment::Camera,
        _ => Moment::Quiet,
    }
}

fn number(moment: Moment) -> u8 {
    match moment {
        Moment::Quiet => 0,
        Moment::Meeting => 1,
        Moment::Sharing => 2,
        Moment::Camera => 3,
    }
}

/// The moment the screen is in (§7.4).
pub fn moment() -> Moment {
    moment_of(MOMENT.load(Ordering::Relaxed))
}

/// The moment as a number: what the drawer lays out again for.
pub fn moment_key() -> u8 {
    MOMENT.load(Ordering::Relaxed)
}

fn send(input: Input) -> bool {
    let gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
    gate.as_ref().is_some_and(|g| g.send(input).is_ok())
}

/// The gate's thread, started once she is there.
fn start() {
    let mut gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
    if gate.is_some() {
        return;
    }
    let (sender, inputs) = mpsc::channel();
    let started = std::thread::Builder::new()
        .name("speak gate".to_owned())
        .spawn(move || run(&inputs));
    match started {
        Ok(_) => {
            *gate = Some(sender);
            drop(gate);
            // The day's changes (a meeting added, moved) look again.
            crate::day::on_change(|| {
                let gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
                gate.as_ref().is_some_and(|g| g.send(Input::Poke).is_ok())
            });
        }
        Err(err) => tracing::warn!(%err, "no thread for the speak gate"),
    }
}

/// She asks to speak.
pub fn asked(speak: Speak) {
    start();
    send(Input::Asked(speak));
}

/// Her restraint's settings.
pub fn restraint(restraint: Restraint) {
    start();
    send(Input::Restraint(restraint));
}

/// Something the screen's state is read from changed: the gate looks
/// again. Nothing without her.
pub fn poke() {
    send(Input::Poke);
}

/// Her peek went back in.
pub fn back(id: u64) {
    send(Input::Back(id));
}

/// Classic again, or she went: the gate stops, what it held with it.
pub fn off() {
    let gate = GATE.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(gate) = gate {
        let _ = gate.send(Input::Stop);
    }
    peek::forget_all();
    if MOMENT.swap(0, Ordering::Relaxed) != 0 {
        crate::drawer::changed();
    }
}

/// The shell's wall clock (`HOCUS_CLOCK_AT` shifts it in tests).
fn now() -> jiff::Zoned {
    crate::notifications::wall()
}

/// The screen now, from what the shell has, and the next moment its
/// meetings change it.
fn screen(now: Timestamp) -> (Screen, Option<Timestamp>) {
    let uses = crate::in_use::snapshot();
    let shared =
        uses.shares.iter().any(|s| s.live()) || uses.recording.as_ref().is_some_and(|r| r.live());
    let camera = uses.camera.iter().any(|c| c.live()) || uses.microphone.iter().any(|m| m.live());
    let dnd = crate::notifications::with(|c| c.stack.dnd.is_on());
    let locked = crate::lock::active() || crate::curtain::with(|c| c.is_some());
    let fullscreen = crate::reminder::fullscreen_any();
    // A meeting of the featured calendars taking time now; the next
    // start or end, when the moment changes.
    let day = crate::day::snapshot();
    let zone = jiff::tz::TimeZone::system();
    let until = now
        .checked_add(SignedDuration::from_hours(24))
        .unwrap_or(now);
    let mut meeting_until = None;
    let mut next = None::<Timestamp>;
    for shown in day.featured(now, until, &zone) {
        if !shown.event.takes_time() {
            continue;
        }
        let (start, end) = (shown.event.when.start(&zone), shown.event.when.end(&zone));
        if start <= now && now < end {
            meeting_until = Some(meeting_until.map_or(end, |e: Timestamp| e.max(end)));
        }
        for at in [start, end] {
            if at > now {
                next = Some(next.map_or(at, |n: Timestamp| n.min(at)));
            }
        }
    }
    (
        Screen {
            meeting_until,
            dnd,
            shared,
            camera,
            locked,
            fullscreen,
            typed: None,
        },
        next,
    )
}

/// The gate's thread: it waits for what changes, or its next moment.
fn run(inputs: &Receiver<Input>) {
    let mut gate = Gate::default();
    let mut wake: Option<Timestamp> = None;
    loop {
        let input = match wake {
            Some(at) => {
                let left = at.duration_since(now().timestamp());
                let left = Duration::try_from(left).unwrap_or(Duration::ZERO);
                match inputs.recv_timeout(left + Duration::from_millis(5)) {
                    Ok(input) => Some(input),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            None => match inputs.recv() {
                Ok(input) => Some(input),
                Err(_) => return,
            },
        };
        let zoned = now();
        let at = zoned.timestamp();
        let local = zoned.datetime();
        let (screen, meetings_change) = screen(at);
        gate.screen = screen;
        let moment = screen.moment(at);
        if MOMENT.swap(number(moment), Ordering::Relaxed) != number(moment) {
            tracing::info!(?moment, "the moment");
            crate::secretary::moment(moment);
            crate::drawer::changed();
        }
        let mut decided = Vec::new();
        match input {
            Some(Input::Stop) => return,
            Some(Input::Restraint(restraint)) => gate.restraint = restraint,
            Some(Input::Asked(speak)) => {
                let verdict = gate.ask(speak.clone(), at, local);
                decided.push((speak, verdict, true));
            }
            Some(Input::Back(id)) => gate.back(id),
            Some(Input::Poke) | None => {}
        }
        for d in gate.step(at, local) {
            decided.push((d.speak, d.verdict, false));
        }
        for (speak, verdict, new) in decided {
            match verdict {
                Verdict::Now => {
                    tracing::info!(
                        id = speak.id,
                        kind = speak.kind.word(),
                        "her peek comes out"
                    );
                    let stays = gate.restraint.stays;
                    peek::ready(speak.clone());
                    crate::popover::send(vec![crate::popover::Effect::Tell(
                        ToCompositor::SecretaryPeek {
                            id: speak.id,
                            stays,
                        },
                    )]);
                    crate::secretary::spoken(speak.id, Spoken::Shown);
                }
                Verdict::Held(why) => {
                    if new {
                        tracing::info!(id = speak.id, why = why.word(), "her peek is held");
                        crate::secretary::spoken(speak.id, Spoken::Held { why });
                    }
                }
                Verdict::Dropped(why) => {
                    tracing::info!(id = speak.id, why = why.word(), "her peek is dropped");
                    crate::secretary::spoken(speak.id, Spoken::Dropped { why });
                }
            }
        }
        wake = gate.next(at).into_iter().chain(meetings_change).min();
    }
}
