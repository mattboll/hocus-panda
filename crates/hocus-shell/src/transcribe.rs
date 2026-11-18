//! A meeting's transcription, the bar's side (docs/design/meetings.md):
//! the meeting card's button, which asks, then stops; when to stop by
//! itself; "Ne plus demander".
//!
//! - **The button** (Day.2's card): while its meeting is in progress and
//!   an application uses the microphone (5.12), it is **Transcrire** in
//!   place of the join button; while that meeting records, a stop.
//! - **Asking**: its popover ([`crate::popover::transcribe`]), unless "Ne
//!   plus demander" was ticked (kept in `$XDG_STATE_HOME/hocuspanda`).
//! - **Stopping**: the card's stop; the meeting's end (the card no longer
//!   its meeting); the call's application away from the microphone for
//!   [`AWAY`]. The compositor records and makes the text; it says the
//!   recording's time (`transcribing`), which the in-use tile counts on
//!   from (`crate::in_use`).

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    IN_USE_NAMESPACE, MEETING_NAMESPACE, ToCompositor, TranscribeAsk, TranscribeMeeting,
    TranscribingShown,
};

use crate::{
    in_use::Media,
    meeting::{Card, Phase},
    popover::{self, Effect, Item},
};

/// The call away from the microphone this long: the recording stops.
pub const AWAY: Duration = Duration::from_secs(120);

/// What the meeting card's button is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// None: no video call, nothing to transcribe.
    None,
    /// Joins its video call.
    Join,
    /// Asks to transcribe it.
    Transcribe,
    /// Stops its transcription.
    Stop,
}

#[derive(Debug, Default)]
struct State {
    /// The recording, as the compositor last said it, and when.
    shown: Option<(TranscribingShown, Instant)>,
    /// A start asked, not yet said back: not asked twice.
    asked: Option<Instant>,
    /// "Ne plus demander", once read.
    never_ask: Option<bool>,
    /// Ticked in the asking popover.
    ticked: bool,
    /// Since when the call's application has not used the microphone.
    away: Option<Instant>,
    /// The button as the card last drew it, by meeting.
    button: Option<(String, Button)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// The recording under way, and when the compositor said it.
pub fn recording() -> Option<(TranscribingShown, Instant)> {
    state(|s| s.shown.clone())
}

/// How long the recording has lasted at `now`.
pub fn elapsed(shown: &TranscribingShown, at: Instant, now: Instant) -> Duration {
    Duration::from_millis(shown.elapsed) + now.saturating_duration_since(at)
}

/// The application in the call: the first using the microphone now.
fn caller() -> Option<Media> {
    crate::in_use::snapshot()
        .microphone
        .into_iter()
        .find(|m| m.live())
        .map(|m| m.what)
}

/// The meeting card's button for `card`.
pub fn button(card: &Card) -> Button {
    let recording = recording();
    if recording
        .as_ref()
        .is_some_and(|(shown, _)| shown.event == card.id)
    {
        return Button::Stop;
    }
    let under = matches!(card.phase, Phase::Under { .. });
    if under && recording.is_none() && caller().is_some() {
        return Button::Transcribe;
    }
    if card.video.is_some() {
        Button::Join
    } else {
        Button::None
    }
}

/// Whether "Ne plus demander" was ticked (read once).
pub fn never_ask() -> bool {
    if let Some(never) = state(|s| s.never_ask) {
        return never;
    }
    let never = hocus_meetings::Places::from_env().is_some_and(|p| p.never_ask().exists());
    state(|s| s.never_ask = Some(never));
    never
}

/// Whether "Ne plus demander" is ticked in the popover.
pub fn ticked() -> bool {
    state(|s| s.ticked)
}

/// The popover's "Ne plus demander" ticked, or not.
pub fn tick(on: bool) {
    state(|s| s.ticked = on);
}

/// The popover opens: nothing ticked.
pub fn asking() {
    state(|s| s.ticked = false);
}

/// "Ne plus demander" kept, on a thread of its own.
fn keep_never_ask() {
    state(|s| s.never_ask = Some(true));
    let Some(places) = hocus_meetings::Places::from_env() else {
        return;
    };
    let kept = std::thread::Builder::new()
        .name("transcribe: never ask".to_owned())
        .spawn(move || {
            let path = places.never_ask();
            let written = std::fs::create_dir_all(&places.state)
                .and_then(|()| std::fs::write(&path, b"Transcrire sans demander.\n"));
            if let Err(err) = written {
                tracing::warn!(%err, path = %path.display(), "\"Ne plus demander\" is not kept");
            }
        });
    if let Err(err) = kept {
        tracing::warn!(%err, "\"Ne plus demander\" is not kept");
    }
}

/// The card's meeting is transcribed: the compositor asked to record it,
/// the call's application's streams as its other track. "Ne plus
/// demander" kept if ticked.
pub fn start() -> Option<Effect> {
    let card = crate::meeting::snapshot()?;
    let app = caller()?;
    let now = Instant::now();
    let fresh = state(|s| {
        let asked = s
            .asked
            .is_some_and(|at| now.duration_since(at) < Duration::from_secs(5));
        if s.shown.is_some() || asked {
            return false;
        }
        s.asked = Some(now);
        s.away = None;
        true
    });
    if !fresh {
        return None;
    }
    if ticked() {
        keep_never_ask();
    }
    crate::meeting::acted_on(&card.id);
    tracing::info!(id = card.id, app = app.key, "the meeting is transcribed");
    Some(Effect::Tell(ToCompositor::Transcribe(
        TranscribeAsk::Start(TranscribeMeeting {
            event: card.id,
            title: card.title,
            start: card.start_at,
            end: card.end_at,
            attendees: card.attendees,
            app: app.key,
        }),
    )))
}

/// Its recording stops (the card's stop).
pub fn stop() -> Option<Effect> {
    recording()?;
    tracing::info!("the meeting's transcription stops");
    Some(Effect::Tell(ToCompositor::Transcribe(TranscribeAsk::Stop)))
}

/// The compositor's `transcribing` (`Some`) and `transcribing-end`.
pub fn set(shown: Option<TranscribingShown>) {
    let now = Instant::now();
    tracing::debug!(transcribing = ?shown, "the meeting's transcription");
    state(|s| {
        if shown.is_some() {
            s.asked = None;
        }
        s.shown = shown.clone().map(|shown| (shown, now));
        s.away = None;
    });
    crate::in_use::set_transcribing(shown);
    refresh();
}

/// The card and its popover draw again when what the button is changed.
pub fn refresh() {
    let button = crate::meeting::snapshot().map(|card| {
        let button = button(&card);
        (card.id, button)
    });
    let changed = state(|s| {
        if s.button == button {
            return false;
        }
        s.button = button.clone();
        true
    });
    if changed {
        tracing::debug!(button = ?button.as_ref().map(|b| b.1), "the meeting card's button");
        popover::send(vec![
            Effect::Wake(MEETING_NAMESPACE),
            Effect::Wake(IN_USE_NAMESPACE),
        ]);
        popover::changed(Item::MeetingMark);
        popover::refit(Item::MeetingMark);
    }
}

/// The meeting card changed: its meeting over (another, or none), its
/// recording stops.
pub fn meeting_changed(card: Option<&Card>) {
    let Some((shown, _)) = recording() else {
        refresh();
        return;
    };
    if card.is_none_or(|c| c.id != shown.event) {
        tracing::info!(
            event = shown.event,
            "the meeting is over: its transcription stops"
        );
        popover::tell(ToCompositor::Transcribe(TranscribeAsk::Stop));
    }
    refresh();
}

/// What uses the microphone changed: the call away from it for 2 min, its
/// recording stops.
pub fn uses_changed() {
    let Some((shown, _)) = recording() else {
        refresh();
        return;
    };
    let here = crate::in_use::snapshot()
        .microphone
        .iter()
        .any(|m| m.live() && m.what.key == shown.app);
    let now = Instant::now();
    let away_since = state(|s| {
        match (here, s.away) {
            (true, _) => s.away = None,
            (false, None) => s.away = Some(now),
            (false, Some(_)) => return None,
        }
        s.away
    });
    if let Some(since) = away_since {
        tracing::debug!(app = shown.app, "the call left the microphone");
        let waited = std::thread::Builder::new()
            .name("transcribe: away".to_owned())
            .spawn(move || {
                std::thread::sleep(AWAY);
                let still = state(|s| s.away == Some(since) && s.shown.is_some());
                if still {
                    tracing::info!(
                        "the call left the microphone 2 min ago: the transcription stops"
                    );
                    popover::tell(ToCompositor::Transcribe(TranscribeAsk::Stop));
                }
            });
        if let Err(err) = waited {
            tracing::warn!(%err, "the call's absence is not timed");
        }
    }
    refresh();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recording_counts_on_from_what_the_compositor_said() {
        let shown = TranscribingShown {
            id: 1,
            event: "p".to_owned(),
            title: "Point produit".to_owned(),
            app: "Firefox".to_owned(),
            elapsed: 61_000,
        };
        let at = Instant::now();
        assert_eq!(
            elapsed(&shown, at, at + Duration::from_secs(2)),
            Duration::from_secs(63)
        );
    }
}
