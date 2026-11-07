//! A day of the desktop's CPU and memory (docs/performance.md, "A day's
//! view").
//!
//! - [`record`]: the counters the compositor's loop only increments, the
//!   ring its events go into, and the thread that wakes every 10 s to
//!   append one sample to the day's file, never touching the loop.
//! - [`format`]: that file, one JSON line per sample, readable after a
//!   crash; [`files`] where the days are kept, 14 of them.
//! - [`signals`]: what counts as a problem, judged sample after sample the
//!   same way while recording and when a day is read.
//! - [`report`]: a day read back: its totals, its peaks with what was
//!   happening, its signals; [`text`] says them, for `hocus perf day` and
//!   the Settings' Performances page.

pub mod files;
pub mod fixture;
pub mod format;
pub mod record;
pub mod report;
pub mod signals;
pub mod text;

pub use files::Day;
pub use format::{Event, EventKind, Sample};
pub use record::{Counters, Recorder, Setup};
pub use report::{Peak, Report};
pub use signals::{Signal, SignalKind, Watch};
pub use text::Lang;
