//! What the effects cost the loop, measured on the live session (#815,
//! docs/performance.md, "The effects' cost"): with `[developer]
//! effects-timing = true`, every 5 s, a line per effect at info level
//! under `hocus_compositor::effects`: its steps a second and the loop's
//! CPU time a step for its tick, the building of its elements and their
//! draws submitted; and one for the frames, whatever they drew.
//!
//! The thread's CPU time, not the clock's: what the loop waits for (the
//! GPU, a fence) is not counted. Off, nothing is measured: a flag read.

use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};

/// How often the lines are written.
const EVERY: Duration = Duration::from_secs(5);

/// The effects measured, in [`Fx::ALL`]'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fx {
    Snow,
    Garland,
    Creatures,
    Rain,
    Koi,
    Fireflies,
    Aurora,
    Daylight,
    Depth,
    Moments,
}

impl Fx {
    const ALL: [Fx; 10] = [
        Fx::Snow,
        Fx::Garland,
        Fx::Creatures,
        Fx::Rain,
        Fx::Koi,
        Fx::Fireflies,
        Fx::Aurora,
        Fx::Daylight,
        Fx::Depth,
        Fx::Moments,
    ];

    fn name(self) -> &'static str {
        match self {
            Fx::Snow => "snow",
            Fx::Garland => "garland",
            Fx::Creatures => "creatures",
            Fx::Rain => "rain-on-glass",
            Fx::Koi => "koi",
            Fx::Fireflies => "fireflies",
            Fx::Aurora => "aurora",
            Fx::Daylight => "daylight",
            Fx::Depth => "depth",
            Fx::Moments => "moments",
        }
    }
}

/// What one effect cost since the last lines (nanoseconds).
#[derive(Debug, Default, Clone, Copy)]
struct Cost {
    steps: u64,
    tick: u64,
    build: u64,
    draw: u64,
}

#[derive(Debug)]
struct Totals {
    since: Instant,
    effects: [Cost; Fx::ALL.len()],
    frames: u64,
    frame_ns: u64,
}

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    /// The effect whose elements are being built.
    static BUILDING: Cell<Option<Fx>> = const { Cell::new(None) };
    /// The measures under way, innermost last: what they measure, when
    /// they started, the time their inner ones took.
    static OPEN: RefCell<Vec<(Stage, u64, u64)>> = const { RefCell::new(Vec::new()) };
    static TOTALS: RefCell<Option<Totals>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy)]
enum Stage {
    Tick(Fx),
    Build(Fx),
    Draw(Fx),
    Frame,
}

/// `[developer] effects-timing` as it now says.
pub fn follow(on: bool) {
    if ON.replace(on) != on {
        TOTALS.set(None);
        tracing::info!(
            target: "hocus_compositor::effects",
            on,
            "the effects' timing"
        );
    }
}

/// The thread's CPU time, nanoseconds.
fn cpu() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    (t.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(t.tv_nsec as u64)
}

/// A measure under way, taken as it drops.
#[must_use]
pub struct Measure(bool);

fn open(stage: Stage) -> Measure {
    if !ON.get() {
        return Measure(false);
    }
    if let Stage::Build(fx) = stage {
        BUILDING.set(Some(fx));
    }
    OPEN.with_borrow_mut(|open| open.push((stage, cpu(), 0)));
    Measure(true)
}

impl Drop for Measure {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        let Some((stage, start, inner)) = OPEN.with_borrow_mut(Vec::pop) else {
            return;
        };
        let took = cpu().saturating_sub(start);
        // Its own time alone: what was measured within it is counted there.
        let own = took.saturating_sub(inner);
        let outer = OPEN.with_borrow_mut(|open| {
            open.last_mut().map(|(stage, _, inner)| {
                *inner += took;
                *stage
            })
        });
        if let Stage::Build(_) = stage {
            BUILDING.set(match outer {
                Some(Stage::Build(fx)) => Some(fx),
                _ => None,
            });
        }
        // A frame's all of it, what was measured within it too.
        add(stage, if let Stage::Frame = stage { took } else { own });
    }
}

/// What is counted since the last lines, begun now if nothing is.
fn totals<T>(f: impl FnOnce(&mut Totals) -> T) -> T {
    TOTALS.with_borrow_mut(|totals| {
        f(totals.get_or_insert_with(|| Totals {
            since: Instant::now(),
            effects: Default::default(),
            frames: 0,
            frame_ns: 0,
        }))
    })
}

fn add(stage: Stage, ns: u64) {
    totals(|totals| {
        let (fx, stage): (Fx, fn(&mut Cost) -> &mut u64) = match stage {
            Stage::Tick(fx) => (fx, |c| &mut c.tick),
            Stage::Build(fx) => (fx, |c| &mut c.build),
            Stage::Draw(fx) => (fx, |c| &mut c.draw),
            Stage::Frame => {
                totals.frames += 1;
                totals.frame_ns += ns;
                return;
            }
        };
        if let Some(cost) = totals.effects.get_mut(fx as usize) {
            *stage(cost) += ns;
        }
    });
}

/// Effect `fx` steps on: measured until it drops.
pub fn tick(fx: Fx) -> Measure {
    open(Stage::Tick(fx))
}

/// Effect `fx` stepped on, moving: a step counted.
pub fn stepped(fx: Fx, moving: bool) {
    if !ON.get() || !moving {
        return;
    }
    totals(|totals| {
        if let Some(cost) = totals.effects.get_mut(fx as usize) {
            cost.steps += 1;
        }
    });
}

/// Effect `fx`'s elements are built: what they draw is counted as its.
pub fn build(fx: Fx) -> Measure {
    open(Stage::Build(fx))
}

/// The effect whose elements are built now, for them to say whose they
/// are as they draw.
pub fn building() -> Option<Fx> {
    if ON.get() { BUILDING.get() } else { None }
}

/// An element of effect `fx` draws.
pub fn draw(fx: Option<Fx>) -> Measure {
    match fx {
        Some(fx) => open(Stage::Draw(fx)),
        None => Measure(false),
    }
}

/// A screen's frame is built and drawn (all of it, what draws the effects
/// within); the lines written when it is time.
pub fn frame() -> Measure {
    say();
    open(Stage::Frame)
}

/// The lines, every [`EVERY`].
fn say() {
    if !ON.get() {
        return;
    }
    TOTALS.with_borrow_mut(|totals| {
        let Some(t) = totals.as_ref() else { return };
        let span = t.since.elapsed();
        if span < EVERY {
            return;
        }
        let secs = span.as_secs_f64();
        for (fx, cost) in Fx::ALL.iter().zip(&t.effects) {
            let all = cost.tick + cost.build + cost.draw;
            // Those that played: off, an effect's tick and builder only
            // look whether it is on.
            if cost.steps == 0 && cost.draw == 0 {
                continue;
            }
            let per = |ns: u64| ns / 1000 / cost.steps.max(1);
            tracing::info!(
                target: "hocus_compositor::effects",
                effect = fx.name(),
                steps_a_second = format!("{:.1}", cost.steps as f64 / secs),
                us_a_step = per(all),
                tick = per(cost.tick),
                build = per(cost.build),
                draw = per(cost.draw),
                "an effect's cost"
            );
        }
        tracing::info!(
            target: "hocus_compositor::effects",
            frames_a_second = format!("{:.1}", t.frames as f64 / secs),
            us_a_frame = t.frame_ns / 1000 / t.frames.max(1),
            "the frames' cost"
        );
        *totals = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_built_within_an_effect_is_its_own() {
        follow(true);
        {
            let _outer = build(Fx::Snow);
            assert_eq!(building(), Some(Fx::Snow));
            {
                let _inner = build(Fx::Garland);
                assert_eq!(building(), Some(Fx::Garland));
            }
            assert_eq!(building(), Some(Fx::Snow));
        }
        assert_eq!(building(), None);
        follow(false);
        assert_eq!(building(), None);
    }
}
