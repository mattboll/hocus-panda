//! The sound, from PipeWire, for the bar's sound popover and the Settings'
//! Son page alike (bar.md §10, settings.md "Son"): the outputs (sinks),
//! the inputs (sources), the devices and their profiles, the streams
//! applications play and record.
//!
//! - [`dump`]: `pw-dump --monitor` (PipeWire's own tool) streams the
//!   graph's changes as JSON, read into a [`Graph`]. No polling: it writes
//!   when something changes.
//! - [`set`]: `wpctl` (WirePlumber's), `pw-cli` and `pw-metadata` set
//!   what the user asks, as they would by hand, what was asked meanwhile
//!   taken together.
//! - [`meter`]: a microphone's level, with `pw-record`.
//! - [`tester`]: a sound on the left then on the right, with `pw-play`.
//!
//! All run as programs, on threads of their own, never on a loop that
//! draws; linking to libpipewire would need its headers to build, for the
//! same result. No smithay, no toolkit.

pub mod dump;
pub mod graph;
pub mod meter;
pub mod set;
pub mod tester;

pub use graph::{App, Card, Graph, Kind, Mixer, Profile, Recorder, Sink, Used, Using};
pub use set::{Ask, Known, Set, Step};

/// A volume and its mute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    /// In percent, as `wpctl` shows it (the cube root of the loudest
    /// channel's linear volume); above 100 when amplified.
    pub volume: f32,
    pub muted: bool,
}

/// The volume keys' step, in percent.
pub const STEP: f32 = 5.0;
