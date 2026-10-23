//! Frame times and slow loop handlers: no slowdown without a name.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use hocus_api::{Bucket, OutputStats, SlowHandler, Stall, Stats};

use crate::App;

/// A handler that runs longer than a frame at 60 Hz makes a frame late.
pub const HANDLER_BUDGET: Duration = Duration::from_millis(16);
/// Upper bounds of the frame time buckets.
const BUCKETS_MS: [f64; 7] = [1.0, 2.0, 4.0, 8.0, 16.0, 33.0, 66.0];
/// How many slow handlers and stalls are kept.
const KEPT: usize = 32;

/// What the compositor measured about itself. Durations are real time: they
/// measure cost, whatever the clock the compositor runs on.
#[derive(Debug, Default)]
pub struct Measures {
    frames: HashMap<String, FrameTimes>,
    /// Blurs computed, per output.
    blurs: HashMap<String, u64>,
    /// Frames of a closing glitch painted with its colours apart, per
    /// output.
    glitches: HashMap<String, u64>,
    slow_handlers: VecDeque<SlowHandler>,
    stalls: VecDeque<Stall>,
}

#[derive(Debug, Default, Clone)]
struct FrameTimes {
    counts: [u64; BUCKETS_MS.len() + 1],
    max: Duration,
}

impl Measures {
    /// An output presented a frame that took `took` to draw.
    pub fn frame(&mut self, output: &str, took: Duration) {
        let times = self.frames.entry(output.to_string()).or_default();
        let ms = took.as_secs_f64() * 1000.0;
        let bucket = BUCKETS_MS
            .iter()
            .position(|&bound| ms <= bound)
            .unwrap_or(BUCKETS_MS.len());
        times.counts[bucket] += 1;
        times.max = times.max.max(took);
    }

    /// The blur behind something on `output` was computed again.
    pub fn blur(&mut self, output: &str) {
        *self.blurs.entry(output.to_string()).or_default() += 1;
    }

    /// A closing window's glitch was painted on `output` with its colours
    /// apart (#169).
    pub fn glitch(&mut self, output: &str) {
        *self.glitches.entry(output.to_string()).or_default() += 1;
    }

    pub fn slow_handler(&mut self, name: &str, took: Duration, at: Duration) {
        push_bounded(
            &mut self.slow_handlers,
            SlowHandler {
                name: name.to_string(),
                ms: took.as_secs_f64() * 1000.0,
                at_ms: at.as_millis() as u64,
            },
        );
    }

    pub fn stall(&mut self, stall: Stall) {
        push_bounded(&mut self.stalls, stall);
    }

    /// The stats of the API, with outputs in the order given.
    pub fn report<'a>(&self, outputs: impl Iterator<Item = &'a str>) -> Stats {
        let outputs = outputs
            .map(|name| {
                let times = self.frames.get(name).cloned().unwrap_or_default();
                let histogram = times
                    .counts
                    .iter()
                    .enumerate()
                    .map(|(i, &count)| Bucket {
                        up_to_ms: BUCKETS_MS.get(i).copied(),
                        count,
                    })
                    .collect();
                OutputStats {
                    name: name.to_string(),
                    frames: times.counts.iter().sum(),
                    histogram,
                    max_ms: times.max.as_secs_f64() * 1000.0,
                    blurs: self.blurs.get(name).copied().unwrap_or(0),
                    glitches: self.glitches.get(name).copied().unwrap_or(0),
                }
            })
            .collect();
        Stats {
            outputs,
            slow_handlers: self.slow_handlers.iter().cloned().collect(),
            stalls: self.stalls.iter().cloned().collect(),
        }
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T) {
    if queue.len() == KEPT {
        queue.pop_front();
    }
    queue.push_back(item);
}

impl App {
    /// Runs a loop handler, and names it in the log and the stats when it
    /// goes over [`HANDLER_BUDGET`].
    pub fn timed<R>(&mut self, name: &'static str, handler: impl FnOnce(&mut App) -> R) -> R {
        let start = Instant::now();
        let result = handler(self);
        let took = start.elapsed();
        if took > HANDLER_BUDGET {
            tracing::warn!(
                handler = name,
                ms = took.as_millis() as u64,
                "loop handler over budget"
            );
            let now = self.state.clock.now();
            self.state.measures.slow_handler(name, took, now);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_times_land_in_their_bucket() {
        let mut measures = Measures::default();
        measures.frame("A", Duration::from_micros(500));
        measures.frame("A", Duration::from_millis(12));
        measures.frame("A", Duration::from_millis(100));
        let stats = measures.report(["A", "B"].into_iter());

        let a = &stats.outputs[0];
        assert_eq!(a.frames, 3);
        let counts: Vec<u64> = a.histogram.iter().map(|b| b.count).collect();
        assert_eq!(counts, [1, 0, 0, 0, 1, 0, 0, 1]);
        assert_eq!(a.histogram.last().map(|b| b.up_to_ms), Some(None));
        assert_eq!(a.max_ms, 100.0);
        assert_eq!(stats.outputs[1].frames, 0);
    }

    #[test]
    fn only_the_latest_slow_handlers_are_kept() {
        let mut measures = Measures::default();
        for i in 0..40 {
            measures.slow_handler("h", Duration::from_millis(20), Duration::from_millis(i));
        }
        let stats = measures.report(std::iter::empty());
        assert_eq!(stats.slow_handlers.len(), KEPT);
        assert_eq!(stats.slow_handlers[0].at_ms, 8);
    }
}
