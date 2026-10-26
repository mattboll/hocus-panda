//! Scenario runner, test clients and screenshot comparison.

pub mod accounts;
pub mod ai;
pub mod atspi;
pub mod bus;
pub mod caldav;
pub mod client;
pub mod devices;
pub mod fuzz;
pub mod goa;
pub mod herdr;
pub mod image;
pub mod layer;
pub mod phone;
pub mod randr;
pub mod scenario;
pub mod screencopy;
pub mod session_lock;
pub mod timedated;
pub mod tray;
pub mod x11;

/// The loop handlers that held the loop: over three times their 16 ms
/// budget. With the whole suite running at once any handler can be
/// preempted for a frame or two; what these checks look for (waiting on a
/// file, a bus, a process) takes far longer. The start is left out: at
/// virtual time 0 the first requests load fonts and the theme and draw the
/// first frames, once, and a slow machine takes over 48 ms for it (#231).
pub fn held_the_loop(stats: &hocus_api::Stats) -> Vec<&hocus_api::SlowHandler> {
    stats
        .slow_handlers
        .iter()
        .filter(|h| h.ms > 48.0 && h.at_ms > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_does_not_count_as_holding_the_loop() {
        let slow = |ms, at_ms| hocus_api::SlowHandler {
            name: "control request".into(),
            ms,
            at_ms,
        };
        let stats = hocus_api::Stats {
            outputs: Vec::new(),
            slow_handlers: vec![slow(53.0, 0), slow(53.0, 120), slow(20.0, 300)],
            stalls: Vec::new(),
            wakeups: 0,
            handlers: Vec::new(),
        };
        let held = held_the_loop(&stats);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].at_ms, 120);
    }
}
