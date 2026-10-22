//! Scenario runner, test clients and screenshot comparison.

pub mod bus;
pub mod caldav;
pub mod client;
pub mod fuzz;
pub mod image;
pub mod layer;
pub mod randr;
pub mod scenario;
pub mod screencopy;
pub mod session_lock;
pub mod tray;
pub mod x11;

/// The loop handlers that held the loop: over three times their 16 ms
/// budget. With the whole suite running at once any handler can be
/// preempted for a frame or two; what these checks look for (waiting on a
/// file, a bus, a process) takes far longer.
pub fn held_the_loop(stats: &hocus_api::Stats) -> Vec<&hocus_api::SlowHandler> {
    stats.slow_handlers.iter().filter(|h| h.ms > 48.0).collect()
}
