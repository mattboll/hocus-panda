//! Where a frame drawn off the screen goes: a share's stream on PipeWire
//! (5.11), or a recording's encoder (5.10). Both take the frame's pixels
//! lent ([`super::lease`]), never copied on the loop, and give them back
//! once they copied them on their own thread.

use super::{
    lease::Lease,
    stream::{FromStreams, StreamsSender, ToStreams},
};
use crate::record::encode::{FromRecorder, RecordSender, ToRecorder};

/// A frame's consumer.
#[derive(Debug, Clone)]
pub enum FrameTo {
    Stream(StreamsSender),
    Record(RecordSender),
}

impl FrameTo {
    /// `pixels`, lent; `quiet`: the lender (the read-back thread) tells the
    /// loop itself once they are free.
    pub fn lend(&self, pixels: &[u8], quiet: bool) -> Lease {
        match self {
            Self::Stream(s) if quiet => s.lend_quiet(pixels),
            Self::Stream(s) => s.lend(pixels),
            Self::Record(r) => r.lend(pixels, quiet),
        }
    }

    /// Frame `id`'s pixels, lent: rows of `stride` bytes, `size` pixels.
    pub fn frame(&self, id: u64, lease: Lease, size: (u32, u32), stride: u32) {
        match self {
            Self::Stream(s) => s.send(ToStreams::Frame {
                id,
                lease,
                size,
                stride,
            }),
            Self::Record(r) => r.send(ToRecorder::Frame {
                lease,
                size,
                stride,
            }),
        }
    }

    /// Frame `id`'s memory may be read into again: the loop is told.
    pub fn returned(&self, id: u64) {
        match self {
            Self::Stream(s) => s.tell(FromStreams::Returned { id }),
            Self::Record(r) => r.tell(FromRecorder::Returned { id: r.id }),
        }
    }
}
