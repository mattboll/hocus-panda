//! What every program of the desktop does when it panics (#778): one
//! line in its log, with the message, the place, the thread and the
//! calls that led there, then what Rust does by default (the message on
//! stderr, the thread unwound).
//!
//! A session's stderr goes to the journal, and the log's lines carry
//! their time and level there: without this line, a compositor that
//! panicked ended the session with nothing to find it by among the
//! others.
//!
//! Nothing is recovered here: the hook only says what happened. What a
//! program does next is its own (the compositor's `run` ends the session
//! as it ends otherwise).

use std::{backtrace::Backtrace, panic::PanicHookInfo};

/// Set (to anything): [`install`] panics at once. The tests of each
/// program's hook start it so, and read its log.
pub const TEST: &str = "HOCUS_TEST_PANIC";

/// The log's directive that lets the panic's line through: a program
/// adds it to its filter, whatever `RUST_LOG` names (`hocus_settings=debug`
/// alone would leave it out).
pub const LOG: &str = "hocus_panic=error";

/// From here on, a panic of any thread is logged before the default hook
/// runs. Call it once the log is set up: a panic before is only on stderr.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(info);
        default(info);
    }));
    if std::env::var_os(TEST).is_some() {
        trial();
    }
}

/// The panic, as an error of the log. A panic while the log itself writes
/// a line is not logged (tracing takes no line from inside one): the
/// default hook still says it on stderr.
fn log(info: &PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("unnamed");
    let message = info.payload_as_str().unwrap_or("not a text");
    // Always: `RUST_BACKTRACE` is not set in a session, and the frames
    // are what tells one `Option::expect` from another.
    let backtrace = Backtrace::force_capture();
    // The frames last: the line's first says what, where and on which
    // thread.
    match info.location() {
        Some(at) => tracing::error!(thread, at = %at, %backtrace, "panic: {message}"),
        None => tracing::error!(thread, %backtrace, "panic: {message}"),
    }
}

/// The tests' panic ([`TEST`]).
#[expect(clippy::panic, reason = "asked for by name, to try the hook")]
fn trial() {
    panic!("{TEST} is set");
}
