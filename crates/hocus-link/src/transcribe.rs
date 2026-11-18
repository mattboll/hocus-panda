//! A meeting's transcription's lines (docs/design/meetings.md): the shell
//! says when to start and stop (the meeting card's button, the meeting's
//! end, the call leaving the microphone); the compositor records the
//! sound off the loop, then has `hocus-transcribe` write the text, and
//! says the recording's time for the bar's tile.
//!
//! - From the shell: `transcribe` [`TranscribeAsk`].
//! - To it: `transcribing` [`TranscribingShown`] as it starts and as the
//!   shell connects, `transcribing-end` as it stops.

use serde::{Deserialize, Serialize};

/// The meeting the user transcribes: its calendar's event, and the
/// application in its call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscribeMeeting {
    /// The event's id (an occurrence's).
    pub event: String,
    pub title: String,
    /// Its start and end, RFC 3339 instants.
    pub start: String,
    pub end: String,
    /// Its calendar's attendees, as named there.
    pub attendees: Vec<String>,
    /// The application using the microphone: its streams' key on
    /// PipeWire (its sandbox's app id, else `application.name`, else its
    /// program), whose output streams are the call's track.
    pub app: String,
}

/// What the shell asks of a meeting's transcription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscribeAsk {
    /// The asking popover's "Transcrire": its recording starts.
    Start(TranscribeMeeting),
    /// The card's stop, the meeting's end, or the call away from the
    /// microphone for 2 min: the recording ends, its text is made.
    Stop,
}

/// A meeting's recording under way: for the card's stop and red dot and
/// the in-use tile's time (milliseconds so far; the shell counts on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscribingShown {
    pub id: u64,
    /// The calendar's event it is of.
    pub event: String,
    pub title: String,
    /// The application in its call, as the shell named it.
    pub app: String,
    pub elapsed: u64,
}
